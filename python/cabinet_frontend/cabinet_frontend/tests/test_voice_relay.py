from __future__ import annotations

import struct
import threading
import unittest
from collections import deque

from cabinet_frontend.diagnostics import ChangeLogger
from cabinet_frontend.voice_relay import (
    VOICE_AUDIO_PAYLOAD_TYPE,
    VOICE_INPUT_AUDIO_TAG,
    VOICE_STATUS_TAG,
    PipeWireCapture,
    VoiceRelay,
    _decode_rtp,
)


class Codec:
    def encode(self, value: object) -> bytes:
        import json

        return json.dumps(value, separators=(",", ":")).encode()

    def decode(self, payload: bytes) -> object:
        import json

        return json.loads(payload)


class Connection:
    def __init__(self, incoming: list[bytes] | None = None) -> None:
        self.incoming = incoming or []
        self.sent: list[bytes] = []

    def send(self, payload: bytes) -> None:
        self.sent.append(payload)

    def recv(self, _length: int) -> bytes:
        return self.incoming.pop(0)

    def close(self) -> None:
        return None


class Capture:
    def __init__(self) -> None:
        self.started = 0
        self.finished = 0
        self.cancelled = 0

    def start(self) -> None:
        self.started += 1

    def finish(self) -> list[int]:
        self.finished += 1
        return [1, -2, 3]

    def cancel(self) -> None:
        self.cancelled += 1


class Playback:
    def __init__(self) -> None:
        self.samples: list[int] = []
        self.finished = 0

    def write(self, samples: list[int]) -> None:
        self.samples.extend(samples)

    def finish(self) -> None:
        self.finished += 1


class RunningProcess:
    def poll(self) -> None:
        return None


class VoiceRelayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.connection = Connection()
        self.capture = Capture()
        self.playback = Playback()
        self.relay = VoiceRelay(self.connection, self.capture, self.playback, Codec())

    def test_ready_status_is_tagged_cbor_message(self) -> None:
        self.relay.send_status("ready")
        self.assertEqual(self.connection.sent[0][0], VOICE_STATUS_TAG)
        self.assertEqual(self.relay.codec.decode(self.connection.sent[0][1:])["status"], "ready")

    def test_ptt_lifecycle_sends_listening_and_chunked_input(self) -> None:
        self.relay.handle_control(
            {
                "protocol_version": 2,
                "session_id": 1,
                "turn_id": 7,
                "state_revision": 9,
                "control": "start_ptt",
            }
        )
        self.relay.handle_control(
            {
                "protocol_version": 2,
                "session_id": 1,
                "turn_id": 7,
                "state_revision": 9,
                "control": "release_ptt",
            }
        )
        self.assertEqual(self.capture.started, 1)
        self.assertEqual(self.capture.finished, 1)
        self.assertEqual(self.connection.sent[0][0], VOICE_STATUS_TAG)
        self.assertEqual(self.connection.sent[1][0], VOICE_INPUT_AUDIO_TAG)
        self.assertTrue(self.relay.codec.decode(self.connection.sent[1][1:])["complete"])

    def test_rtp_l16_decodes_big_endian_samples(self) -> None:
        packet = (
            b"\x80"
            + bytes([VOICE_AUDIO_PAYLOAD_TYPE])
            + struct.pack(">HII", 4, 10, 20)
            + struct.pack(">2h", 1000, -1000)
        )
        self.assertEqual(_decode_rtp(packet), (4, 10, 20, False, [1000, -1000]))
        self.relay.handle_datagram(packet)
        self.assertEqual(self.playback.samples, [1000, -1000])

    def test_rtp_padding_is_removed_before_decoding(self) -> None:
        packet = (
            b"\xA0"
            + bytes([VOICE_AUDIO_PAYLOAD_TYPE])
            + struct.pack(">HII", 4, 10, 20)
            + struct.pack(">h", 1000)
            + b"\x00\x01\x02\x04"
        )
        self.assertEqual(_decode_rtp(packet)[-1], [1000])

    def test_control_from_wrong_session_is_ignored(self) -> None:
        self.relay.handle_control(
            {
                "protocol_version": 2,
                "session_id": 99,
                "turn_id": 1,
                "state_revision": 1,
                "control": "start_ptt",
            }
        )
        self.assertEqual(self.capture.started, 0)

    def test_pipewire_ptt_marks_boundary_without_clearing_continuous_ring(self) -> None:
        capture = PipeWireCapture.__new__(PipeWireCapture)
        capture.process = RunningProcess()
        capture.error = None
        capture.samples = deque([1, 2, 3], maxlen=10)
        capture.lock = threading.Lock()
        capture.sample_count = 3
        capture.capture_start_count = None
        capture.diagnostics = ChangeLogger(None)

        capture.start()

        self.assertEqual(list(capture.samples), [1, 2, 3])
        self.assertEqual(capture.capture_start_count, 3)

    def test_pipewire_release_sends_only_samples_since_ptt_boundary(self) -> None:
        capture = PipeWireCapture.__new__(PipeWireCapture)
        capture.process = RunningProcess()
        capture.error = None
        capture.samples = deque([1, 2, 3, 4, 5], maxlen=10)
        capture.lock = threading.Lock()
        capture.sample_count = 5
        capture.capture_start_count = 2
        capture.diagnostics = ChangeLogger(None)

        self.assertEqual(capture.finish(), [3, 4, 5])
        self.assertIsNone(capture.capture_start_count)
