from __future__ import annotations

import unittest

from hardware_frontend.protocol import (
    MAX_FRAME_SIZE,
    CborCodec,
    ProtocolValidationError,
    frame,
    receive_exact,
    receive_frame,
    validate_input_message,
)


class PartialConnection:
    def __init__(self, payload: bytes, chunk_size: int = 1) -> None:
        self.payload = payload
        self.chunk_size = chunk_size

    def recv(self, length: int) -> bytes:
        if not self.payload:
            return b""
        count = min(length, self.chunk_size, len(self.payload))
        result, self.payload = self.payload[:count], self.payload[count:]
        return result


class ProtocolTests(unittest.TestCase):
    def test_frame_uses_big_endian_length_prefix(self) -> None:
        self.assertEqual(frame(b"hello"), b"\x00\x00\x00\x05hello")

    def test_receive_frame_handles_partial_reads(self) -> None:
        connection = PartialConnection(frame(b"complete payload"), chunk_size=2)
        self.assertEqual(receive_frame(connection), b"complete payload")

    def test_receive_exact_rejects_closed_connection(self) -> None:
        with self.assertRaises(ConnectionError):
            receive_exact(PartialConnection(b"x"), 2)

    def test_frame_rejects_payload_over_protocol_limit(self) -> None:
        with self.assertRaises(ValueError):
            frame(b"x" * (MAX_FRAME_SIZE + 1))

    def test_backend_port_zero_is_a_valid_subscriber_port(self) -> None:
        message = {
            "protocol_version": 1,
            "input_sequence": 1,
            "expected_state_revision": 0,
            "input": {
                "cord_topology": [{"first": "subscriber_0", "second": "operator"}],
                "held_controls": {
                    "ptt": False,
                    "police": False,
                    "ems": False,
                    "fire": False,
                    "tap_1": False,
                    "tap_2": False,
                },
                "directory_digits": [0, 0, 0, 1],
                "crank_rotation_timestamps": [],
                "tuning": {"coarse": 0, "fine": 0},
                "debug": {
                    "firmware_version": "test",
                    "transport_connected": True,
                    "device_faults": [],
                },
            },
        }
        validate_input_message(message)

    def test_invalid_topology_is_rejected_before_transmission(self) -> None:
        message = {
            "protocol_version": 1,
            "input_sequence": 1,
            "expected_state_revision": 0,
            "input": {
                "cord_topology": [
                    {"first": "subscriber_0", "second": "operator"},
                    {"first": "subscriber_0", "second": "subscriber_1"},
                ],
                "held_controls": {
                    "ptt": False,
                    "police": False,
                    "ems": False,
                    "fire": False,
                    "tap_1": False,
                    "tap_2": False,
                },
                "directory_digits": [0, 0, 0, 1],
                "crank_rotation_timestamps": [],
                "tuning": {"coarse": 0, "fine": 0},
                "debug": {
                    "firmware_version": "test",
                    "transport_connected": True,
                    "device_faults": [],
                },
            },
        }
        with self.assertRaises(ProtocolValidationError):
            validate_input_message(message)

    def test_cbor_codec_round_trips_when_uv_dependencies_are_installed(self) -> None:
        try:
            codec = CborCodec()
        except RuntimeError as error:
            self.skipTest(str(error))
        value = {"protocol_version": 1, "input_sequence": 1, "accepted": True}
        self.assertEqual(codec.decode(codec.encode(value)), value)
