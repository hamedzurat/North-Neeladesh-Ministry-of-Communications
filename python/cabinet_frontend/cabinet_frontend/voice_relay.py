"""Python audio relay for the Cabinet-side voice transport."""

from __future__ import annotations

import os
import shlex
import socket
import struct
import subprocess
import threading
import time
from collections.abc import Sequence
from typing import Any, Protocol

from .protocol import CborCodec

VOICE_PROTOCOL_VERSION = 2
VOICE_INPUT_SAMPLE_RATE = 16_000
VOICE_INPUT_PACKET_SAMPLES = 320
VOICE_AUDIO_SAMPLE_RATE = 24_000
VOICE_AUDIO_PACKET_SAMPLES = 480
VOICE_AUDIO_PAYLOAD_TYPE = 96
VOICE_STATUS_TAG = 0x01
VOICE_CONTROL_TAG = 0x02
VOICE_INPUT_AUDIO_TAG = 0x03
MAX_CAPTURE_SAMPLES = VOICE_INPUT_SAMPLE_RATE * 15


class Capture(Protocol):
    def start(self) -> None: ...

    def finish(self) -> list[int]: ...

    def cancel(self) -> None: ...


class Playback(Protocol):
    def write(self, samples: Sequence[int]) -> None: ...

    def finish(self) -> None: ...


def _command(value: str | None, default: str) -> list[str]:
    return shlex.split(value) if value and value.strip() else shlex.split(default)


class AlsaCapture:
    """Capture raw little-endian PCM from arecord without Python audio wheels."""

    def __init__(self, command: str | None = None, max_samples: int = MAX_CAPTURE_SAMPLES) -> None:
        self.command = _command(
            command,
            "arecord -q -t raw -f S16_LE -c 1 -r 16000 -",
        )
        self.max_bytes = max_samples * 2
        self.process: subprocess.Popen[bytes] | None = None
        self.buffer = bytearray()
        self.reader: threading.Thread | None = None
        self.error: BaseException | None = None

    def start(self) -> None:
        if self.process is not None:
            raise RuntimeError("microphone is already capturing")
        self.buffer.clear()
        self.error = None
        self.process = subprocess.Popen(
            self.command,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        assert self.process.stdout is not None

        def read() -> None:
            try:
                while True:
                    chunk = self.process.stdout.read(8192)
                    if not chunk:
                        return
                    remaining = self.max_bytes - len(self.buffer)
                    self.buffer.extend(chunk[:remaining])
                    if len(self.buffer) >= self.max_bytes:
                        return
            except OSError as error:  # pragma: no cover - OS failure path
                self.error = error

        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()

    def finish(self) -> list[int]:
        process = self.process
        if process is None:
            raise RuntimeError("microphone was not started")
        process.terminate()
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        if self.reader is not None:
            self.reader.join(timeout=1)
        self.process = None
        if self.error is not None:
            raise RuntimeError(f"microphone read failed: {self.error}")
        if process.returncode not in (0, -15):
            raise RuntimeError(f"arecord exited with {process.returncode}")
        usable = len(self.buffer) - len(self.buffer) % 2
        return list(struct.unpack(f"<{usable // 2}h", self.buffer[:usable]))

    def cancel(self) -> None:
        if self.process is not None:
            self.process.kill()
            self.process.wait()
            self.process = None
        self.buffer.clear()


class AlsaPlayback:
    """Play raw little-endian PCM through aplay."""

    def __init__(self, command: str | None = None) -> None:
        self.command = _command(
            command,
            "aplay -q -t raw -f S16_LE -c 1 -r 24000 -",
        )
        self.process: subprocess.Popen[bytes] | None = None

    def _ensure_started(self) -> subprocess.Popen[bytes]:
        if self.process is None:
            self.process = subprocess.Popen(self.command, stdin=subprocess.PIPE, stderr=subprocess.DEVNULL)
        return self.process

    def write(self, samples: Sequence[int]) -> None:
        process = self._ensure_started()
        assert process.stdin is not None
        payload = struct.pack(f"<{len(samples)}h", *samples)
        try:
            process.stdin.write(payload)
            process.stdin.flush()
        except BrokenPipeError as error:
            raise RuntimeError("speaker playback failed") from error

    def finish(self) -> None:
        if self.process is None:
            return
        process = self.process
        self.process = None
        assert process.stdin is not None
        process.stdin.close()
        try:
            return_code = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            return_code = process.wait()
        if return_code != 0:
            raise RuntimeError(f"aplay exited with {return_code}")


def _tagged(codec: CborCodec, tag: int, value: dict[str, Any]) -> bytes:
    return bytes([tag]) + codec.encode(value)


def _decode_rtp(datagram: bytes) -> tuple[int, int, int, bool, list[int]]:
    if len(datagram) < 12 or datagram[0] >> 6 != 2:
        raise ValueError("invalid RTP packet")
    if datagram[1] & 0x7F != VOICE_AUDIO_PAYLOAD_TYPE:
        raise ValueError("unsupported RTP payload type")
    csrc_count = datagram[0] & 0x0F
    header_length = 12 + csrc_count * 4
    if len(datagram) < header_length:
        raise ValueError("invalid RTP header")
    if datagram[0] & 0x10:
        if len(datagram) < header_length + 4:
            raise ValueError("invalid RTP extension")
        extension_length = struct.unpack_from(">H", datagram, header_length + 2)[0] * 4
        header_length += 4 + extension_length
    if len(datagram) < header_length:
        raise ValueError("invalid RTP PCM payload")
    payload = datagram[header_length:]
    if datagram[0] & 0x20:
        padding = payload[-1] if payload else 0
        if padding == 0 or padding > len(payload):
            raise ValueError("invalid RTP padding")
        payload = payload[:-padding]
    if len(payload) % 2:
        raise ValueError("invalid RTP PCM payload")
    sequence, timestamp, ssrc = struct.unpack_from(">HII", datagram, 2)
    samples = list(struct.unpack(f">{len(payload) // 2}h", payload))
    return sequence, timestamp, ssrc, bool(datagram[1] & 0x80), samples


class VoiceRelay:
    def __init__(
        self,
        connection: Any,
        capture: Capture,
        playback: Playback,
        codec: CborCodec | None = None,
    ) -> None:
        self.connection = connection
        self.capture = capture
        self.playback = playback
        self.codec = codec or CborCodec()
        self.session_id = 1
        self.turn_id = 1
        self.state_revision = 0
        self.last_sequence: int | None = None

    def send_status(
        self,
        status: str,
        transcript: str | None = None,
        response_text: str | None = None,
        error: dict[str, str] | None = None,
    ) -> None:
        self.connection.send(
            _tagged(
                self.codec,
                VOICE_STATUS_TAG,
                {
                    "protocol_version": VOICE_PROTOCOL_VERSION,
                    "session_id": self.session_id,
                    "turn_id": self.turn_id,
                    "state_revision": self.state_revision,
                    "status": status,
                    "transcript": transcript,
                    "response_text": response_text,
                    "error": error,
                },
            )
        )

    def send_input_audio(self, samples: Sequence[int]) -> None:
        chunks = [samples[index : index + VOICE_INPUT_PACKET_SAMPLES] for index in range(0, len(samples), VOICE_INPUT_PACKET_SAMPLES)]
        if not chunks:
            chunks = [[]]
        for index, chunk in enumerate(chunks):
            self.connection.send(
                _tagged(
                    self.codec,
                    VOICE_INPUT_AUDIO_TAG,
                    {
                        "protocol_version": VOICE_PROTOCOL_VERSION,
                        "session_id": self.session_id,
                        "turn_id": self.turn_id,
                        "state_revision": self.state_revision,
                        "chunk_index": index,
                        "complete": index == len(chunks) - 1,
                        "samples": list(chunk),
                    },
                )
            )
            if index + 1 < len(chunks):
                time.sleep(0.001)

    def handle_control(self, message: dict[str, Any]) -> None:
        if message.get("protocol_version") != VOICE_PROTOCOL_VERSION or message.get("session_id") != self.session_id:
            return
        self.turn_id = int(message["turn_id"])
        self.state_revision = int(message["state_revision"])
        control = message.get("control")
        try:
            if control == "start_ptt":
                self.capture.start()
                self.send_status("listening")
            elif control == "release_ptt":
                samples = self.capture.finish()
                self.send_input_audio(samples)
            elif control == "cancel":
                self.capture.cancel()
                self.send_status("cancelled")
        except Exception as error:  # noqa: BLE001 - report device failures over the wire
            self.send_status("failed", error={"code": "audio_device_failed", "message": str(error)})

    def handle_datagram(self, datagram: bytes) -> None:
        if not datagram:
            return
        if datagram[0] == VOICE_CONTROL_TAG:
            self.handle_control(self.codec.decode(datagram[1:]))
        elif datagram[0] == VOICE_STATUS_TAG:
            status = self.codec.decode(datagram[1:])
            if (
                status.get("protocol_version") != VOICE_PROTOCOL_VERSION
                or status.get("session_id") != self.session_id
                or status.get("turn_id") != self.turn_id
            ):
                return
            self.state_revision = int(status.get("state_revision", self.state_revision))
            if status.get("status") in {"completed", "failed", "cancelled"}:
                self.playback.finish()
        else:
            sequence, _timestamp, _ssrc, _marker, samples = _decode_rtp(datagram)
            self.last_sequence = sequence
            self.playback.write(samples)

    def run(self) -> None:
        self.send_status("ready")
        while True:
            try:
                self.handle_datagram(self.connection.recv(65_535))
            except TimeoutError:
                self.send_status("ready")

    def close(self) -> None:
        self.capture.cancel()
        self.playback.finish()
        self.connection.close()


def run_forever() -> None:
    address = os.environ.get("NN_VOICE_BACKEND_ADDRESS", "127.0.0.1:7879")
    host, port_text = address.rsplit(":", 1)
    capture = os.environ.get("NN_VOICE_CAPTURE_COMMAND")
    playback = os.environ.get("NN_VOICE_PLAYBACK_COMMAND")
    while True:
        connection: socket.socket | None = None
        relay: VoiceRelay | None = None
        try:
            connection = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            connection.connect((host, int(port_text)))
            connection.settimeout(5.0)
            relay = VoiceRelay(connection, AlsaCapture(capture), AlsaPlayback(playback))
            relay.run()
        except KeyboardInterrupt:
            return
        except Exception as error:  # noqa: BLE001 - daemon recovery loop
            print(f"VOICE RELAY OFFLINE // {error}", flush=True)
        finally:
            if relay is not None:
                relay.close()
            elif connection is not None:
                connection.close()


if __name__ == "__main__":
    run_forever()
