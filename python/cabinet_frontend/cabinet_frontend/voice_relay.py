"""Python audio relay for the Cabinet-side voice transport."""

from __future__ import annotations

import os
import shlex
import socket
import struct
import subprocess
import threading
import time
from collections import deque
from collections.abc import Sequence
from typing import Any, Protocol

from .diagnostics import ChangeLogger
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


class PipeWireCapture:
    """Keep a PipeWire microphone stream open and retain a bounded PCM ring."""

    def __init__(
        self,
        max_samples: int = MAX_CAPTURE_SAMPLES,
        status_sink: Any = print,
    ) -> None:
        self.command = _command(
            None,
            "pw-record --raw --format s16 --channels 1 --rate 16000 -",
        )
        self.max_samples = max_samples
        self.process: subprocess.Popen[bytes] | None = None
        self.reader: threading.Thread | None = None
        self.samples: deque[int] = deque(maxlen=max_samples)
        self.lock = threading.Lock()
        self.error: BaseException | None = None
        self.sample_count = 0
        self.capture_start_count: int | None = None
        self.diagnostics = ChangeLogger(status_sink)
        self._start_worker()

    def _start_worker(self) -> None:
        self.process = subprocess.Popen(
            self.command,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            bufsize=0,
        )
        assert self.process.stdout is not None
        process = self.process
        self.diagnostics.emit("stream", "recording", "VOICE CAPTURE // continuous recording active")

        def read() -> None:
            try:
                while True:
                    chunk = process.stdout.read(8192)
                    if not chunk:
                        return_code = process.poll()
                        if return_code is not None:
                            self.error = RuntimeError(f"pw-record exited with {return_code}")
                            self.diagnostics.emit(
                                "stream_failure",
                                return_code,
                                f"VOICE CAPTURE FAILED // pw-record exited with {return_code}",
                            )
                        return
                    usable = len(chunk) - len(chunk) % 2
                    values = struct.unpack(f"<{usable // 2}h", chunk[:usable])
                    with self.lock:
                        self.samples.extend(values)
                        self.sample_count += len(values)
            except OSError as error:  # pragma: no cover - OS failure path
                self.error = error

        self.reader = threading.Thread(target=read, daemon=True, name="pipewire-capture")
        self.reader.start()

    def start(self) -> None:
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("PipeWire microphone stream is not running")
        if self.error is not None:
            raise RuntimeError(f"PipeWire microphone read failed: {self.error}")
        with self.lock:
            self.capture_start_count = self.sample_count
        self.diagnostics.emit(
            "ptt_capture",
            "recording",
            "VOICE CAPTURE // PTT boundary marked; continuous recording remains active",
        )

    def finish(self) -> list[int]:
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("PipeWire microphone stream is not running")
        if self.error is not None:
            raise RuntimeError(f"PipeWire microphone read failed: {self.error}")
        with self.lock:
            start_count = self.capture_start_count
            if start_count is None:
                raise RuntimeError("microphone was not started")
            requested = self.sample_count - start_count
            available = min(max(0, requested), len(self.samples))
            values = list(self.samples)[-available:] if available else []
            self.capture_start_count = None
        self.diagnostics.emit(
            "ptt_capture",
            "released",
            f"VOICE CAPTURE // PTT segment ready samples={len(values)}",
        )
        return values

    def cancel(self) -> None:
        with self.lock:
            self.capture_start_count = None
        self.diagnostics.emit("ptt_capture", "cancelled", "VOICE CAPTURE // PTT segment cancelled")

    def close(self) -> None:
        process = self.process
        self.process = None
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        if self.reader is not None:
            self.reader.join(timeout=1)
        self.diagnostics.emit("stream", "closed", "VOICE CAPTURE // continuous recording stopped")


class SoundDeviceCapture:
    """Continuous callback-based capture through PortAudio/PipeWire."""

    def __init__(self, max_samples: int = MAX_CAPTURE_SAMPLES, status_sink: Any = print) -> None:
        try:
            import sounddevice
        except ImportError as error:
            raise RuntimeError("sounddevice is not installed") from error

        self.sounddevice = sounddevice
        self.max_samples = max_samples
        self.samples: deque[int] = deque(maxlen=max_samples)
        self.lock = threading.Lock()
        self.sample_count = 0
        self.capture_start_count: int | None = None
        self.error: BaseException | None = None
        self.stream: Any = None
        self.diagnostics = ChangeLogger(status_sink)
        self._start_stream()

    def _start_stream(self) -> None:
        def callback(indata: Any, _frames: int, _time_info: Any, status: Any) -> None:
            if status:
                self.diagnostics.emit("stream_status", str(status), f"VOICE CAPTURE // {status}")
            try:
                values = memoryview(indata).cast("h")
                with self.lock:
                    self.samples.extend(values)
                    self.sample_count += len(values)
            except Exception as error:  # noqa: BLE001 - callback failures must be surfaced
                self.error = error

        try:
            self.stream = self.sounddevice.RawInputStream(
                samplerate=VOICE_INPUT_SAMPLE_RATE,
                channels=1,
                dtype="int16",
                blocksize=VOICE_INPUT_PACKET_SAMPLES,
                callback=callback,
            )
            self.stream.start()
        except Exception as error:
            self.stream = None
            raise RuntimeError(f"PortAudio capture setup failed: {error}") from error
        self.diagnostics.emit("stream", "recording", "VOICE CAPTURE // library stream active")

    def start(self) -> None:
        if self.stream is None or not self.stream.active:
            raise RuntimeError("PortAudio microphone stream is not running")
        if self.error is not None:
            raise RuntimeError(f"PortAudio microphone read failed: {self.error}")
        with self.lock:
            self.capture_start_count = self.sample_count
        self.diagnostics.emit(
            "ptt_capture",
            "recording",
            "VOICE CAPTURE // PTT boundary marked; continuous recording remains active",
        )

    def finish(self) -> list[int]:
        if self.stream is None or not self.stream.active:
            raise RuntimeError("PortAudio microphone stream is not running")
        if self.error is not None:
            raise RuntimeError(f"PortAudio microphone read failed: {self.error}")
        with self.lock:
            if self.capture_start_count is None:
                raise RuntimeError("microphone was not started")
            requested = self.sample_count - self.capture_start_count
            available = min(max(0, requested), len(self.samples))
            values = list(self.samples)[-available:] if available else []
            self.capture_start_count = None
        self.diagnostics.emit(
            "ptt_capture",
            "released",
            f"VOICE CAPTURE // PTT segment ready samples={len(values)}",
        )
        return values

    def cancel(self) -> None:
        with self.lock:
            self.capture_start_count = None
        self.diagnostics.emit("ptt_capture", "cancelled", "VOICE CAPTURE // PTT segment cancelled")

    def close(self) -> None:
        if self.stream is not None:
            self.stream.stop()
            self.stream.close()
            self.stream = None
        self.diagnostics.emit("stream", "closed", "VOICE CAPTURE // library stream stopped")


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


class SoundDevicePlayback:
    """Play PCM through the same PortAudio device layer as microphone capture."""

    def __init__(self, status_sink: Any = print) -> None:
        try:
            import sounddevice
        except ImportError as error:
            raise RuntimeError("sounddevice is not installed") from error

        self.sounddevice = sounddevice
        self.stream: Any = None
        self.diagnostics = ChangeLogger(status_sink)
        try:
            self.stream = sounddevice.RawOutputStream(
                samplerate=VOICE_AUDIO_SAMPLE_RATE,
                channels=1,
                dtype="int16",
                blocksize=VOICE_AUDIO_PACKET_SAMPLES,
            )
            self.stream.start()
        except Exception as error:
            self.stream = None
            raise RuntimeError(f"PortAudio playback setup failed: {error}") from error

    def write(self, samples: Sequence[int]) -> None:
        if self.stream is None or not self.stream.active:
            raise RuntimeError("PortAudio speaker stream is not running")
        try:
            self.stream.write(struct.pack(f"<{len(samples)}h", *samples))
        except Exception as error:
            raise RuntimeError("speaker playback failed") from error

    def finish(self) -> None:
        stream = self.stream
        self.stream = None
        if stream is None:
            return
        try:
            stream.stop()
        finally:
            stream.close()
        self.diagnostics.emit("stream", "closed", "VOICE PLAYBACK // library stream stopped")


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
        status_sink: Any = None,
        upload_connection: Any | None = None,
    ) -> None:
        self.connection = connection
        self.capture = capture
        self.playback = playback
        self.upload_connection = upload_connection
        self.codec = codec or CborCodec()
        self.session_id = 1
        self.turn_id = 1
        self.state_revision = 0
        self.last_sequence: int | None = None
        self.last_ssrc: int | None = None
        self.closed = False
        self.diagnostics = ChangeLogger(status_sink)

    def send_status(
        self,
        status: str,
        transcript: str | None = None,
        response_text: str | None = None,
        error: dict[str, str] | None = None,
    ) -> None:
        self.diagnostics.emit(
            "local_status",
            (status, transcript, response_text, error),
            "VOICE // " + status + (f" error={error}" if error else ""),
        )
        self.connection.send(
            _tagged(
                self.codec,
                VOICE_STATUS_TAG,
                {
                    "protocol_version": VOICE_PROTOCOL_VERSION,
                    "session_id": self.session_id,
                        "turn_id": self.turn_id,
                        "state_revision": self.state_revision,
                        "sent_at_unix_us": time.time_ns() // 1_000,
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
            datagram = _tagged(
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
            if self.upload_connection is None:
                self.connection.send(datagram)
            else:
                frame = struct.pack(">I", len(datagram)) + datagram
                self.upload_connection.sendall(frame)
                if self.upload_connection.recv(1) != b"\x01":
                    raise ConnectionError("voice upload was not acknowledged")
            if index + 1 < len(chunks):
                time.sleep(0.001)

    def handle_control(self, message: dict[str, Any]) -> None:
        if message.get("protocol_version") != VOICE_PROTOCOL_VERSION or message.get("session_id") != self.session_id:
            self.diagnostics.emit(
                "ignored_control",
                (message.get("protocol_version"), message.get("session_id")),
                "VOICE // ignored control from an unknown session",
            )
            return
        self.turn_id = int(message["turn_id"])
        self.state_revision = int(message["state_revision"])
        control = message.get("control")
        try:
            if control == "start_ptt":
                self.capture.start()
                self.send_status("listening")
            elif control == "release_ptt":
                try:
                    samples = self.capture.finish()
                except RuntimeError as error:
                    # A very short tap can race the start edge. Treat the
                    # unmatched release as an empty turn, not a device fault.
                    if "not started" not in str(error).lower():
                        raise
                    samples = []
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
                self.diagnostics.emit(
                    "ignored_status",
                    (
                        status.get("protocol_version"),
                        status.get("session_id"),
                        status.get("turn_id"),
                    ),
                    "VOICE // ignored status from an obsolete turn",
                )
                return
            self.state_revision = int(status.get("state_revision", self.state_revision))
            self.diagnostics.emit(
                "backend_status",
                (
                    status.get("status"),
                    status.get("error"),
                    status.get("transcript"),
                    status.get("response_text"),
                ),
                "VOICE BACKEND // "
                f"{status.get('status', 'unknown')}"
                + (f" error={status['error']}" if status.get("error") else "")
                + (f" transcript={status['transcript']}" if status.get("transcript") else "")
                + (f" response={status['response_text']}" if status.get("response_text") else ""),
            )
            if status.get("status") in {"completed", "failed", "cancelled"}:
                self.playback.finish()
        else:
            sequence, _timestamp, ssrc, _marker, samples = _decode_rtp(datagram)
            if self.last_ssrc != ssrc:
                self.diagnostics.emit("rtp_ssrc", ssrc, f"VOICE RTP // stream={ssrc}")
            if self.last_sequence is not None and sequence != (self.last_sequence + 1) & 0xFFFF:
                self.diagnostics.emit(
                    "rtp_sequence_gap",
                    (self.last_sequence, sequence),
                    "VOICE RTP // sequence gap "
                    f"expected={(self.last_sequence + 1) & 0xFFFF} received={sequence}",
                )
            self.last_ssrc = ssrc
            self.last_sequence = sequence
            self.playback.write(samples)

    def run(self, stop: threading.Event | None = None) -> None:
        self.send_status("ready")
        while not self.closed and not (stop is not None and stop.is_set()):
            try:
                self.handle_datagram(self.connection.recv(65_535))
            except TimeoutError:
                if not self.closed and not (stop is not None and stop.is_set()):
                    self.send_status("ready")

    def close(self) -> None:
        self.closed = True
        self.capture.cancel()
        close_capture = getattr(self.capture, "close", None)
        if close_capture is not None:
            close_capture()
        self.playback.finish()
        self.connection.close()
        if self.upload_connection is not None:
            self.upload_connection.close()


def _capture() -> Capture:
    return SoundDeviceCapture(status_sink=print)


def run_embedded(
    stop: threading.Event,
    voice_upload_address: tuple[str, int] | None = None,
) -> None:
    """Run the audio edge inside the Cabinet Frontend process.

    The game input loop remains the source of truth for PTT, Police, and EMS.
    The backend turns those held-control edges into UDP controls, and this
    thread captures immediately when that control arrives. It is deliberately
    part of the frontend process rather than a separately launched daemon.
    """
    backend_host = os.environ.get("NN_BACKEND_HOST")
    backend_address = os.environ.get("NN_BACKEND_ADDRESS")
    default_host = backend_host or (backend_address.rsplit(":", 1)[0] if backend_address else "127.0.0.1")
    address = os.environ.get("NN_VOICE_BACKEND_ADDRESS", f"{default_host}:7879")
    host, port_text = address.rsplit(":", 1)
    diagnostics = ChangeLogger(print)
    while not stop.is_set():
        connection: socket.socket | None = None
        upload_connection: socket.socket | None = None
        relay: VoiceRelay | None = None
        try:
            connection = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            connection.connect((host, int(port_text)))
            connection.settimeout(5.0)
            if voice_upload_address is None:
                upload_host, upload_port = default_host, 7881
            else:
                upload_host, upload_port = voice_upload_address
            upload_connection = socket.create_connection((upload_host, upload_port), timeout=5.0)
            relay = VoiceRelay(
                connection,
                _capture(),
                SoundDevicePlayback(status_sink=print),
                upload_connection=upload_connection,
                status_sink=print,
            )
            relay.run(stop)
        except Exception as error:  # noqa: BLE001 - keep hardware frontend alive
            if not stop.is_set():
                diagnostics.emit(
                    "relay_transport",
                    (type(error).__name__, str(error)),
                    f"CABINET VOICE OFFLINE // {type(error).__name__}: {error}",
                )
        finally:
            if relay is not None:
                relay.close()
            elif connection is not None:
                connection.close()
            if upload_connection is not None:
                upload_connection.close()
        stop.wait(1.0)


def run_forever() -> None:
    address = os.environ.get("NN_VOICE_BACKEND_ADDRESS", "127.0.0.1:7879")
    host, port_text = address.rsplit(":", 1)
    capture = os.environ.get("NN_VOICE_CAPTURE_COMMAND")
    playback = os.environ.get("NN_VOICE_PLAYBACK_COMMAND")
    diagnostics = ChangeLogger(print)
    while True:
        connection: socket.socket | None = None
        relay: VoiceRelay | None = None
        try:
            connection = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            connection.connect((host, int(port_text)))
            connection.settimeout(5.0)
            relay = VoiceRelay(
                connection,
                _capture(capture),
                AlsaPlayback(playback),
                status_sink=print,
            )
            relay.run()
        except KeyboardInterrupt:
            return
        except Exception as error:  # noqa: BLE001 - daemon recovery loop
            diagnostics.emit(
                "relay_transport",
                (type(error).__name__, str(error)),
                f"VOICE RELAY OFFLINE // {type(error).__name__}: {error}",
            )
        finally:
            if relay is not None:
                relay.close()
            elif connection is not None:
                connection.close()


if __name__ == "__main__":
    run_forever()
