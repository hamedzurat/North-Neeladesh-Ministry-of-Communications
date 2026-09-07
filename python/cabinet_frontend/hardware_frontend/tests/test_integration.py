from __future__ import annotations

import json
import socket
import threading
import unittest

from hardware_frontend.input_mapper import DummyInputSource
from hardware_frontend.main import HardwareFrontend
from hardware_frontend.output_mapper import OutputMapper
from hardware_frontend.protocol import BackendClient, receive_frame, send_frame
from hardware_frontend.state import PhysicalInput


class JsonCodec:
    def encode(self, value: object) -> bytes:
        return json.dumps(value, separators=(",", ":")).encode()

    def decode(self, payload: bytes) -> object:
        return json.loads(payload)


class Spy:
    def __init__(self) -> None:
        self.calls: list[object] = []

    def set_lines(self, value: object) -> None:
        self.calls.append(("lines", value))

    def show(self, value: object) -> None:
        self.calls.append(("seven", value))

    def show_directory(self, value: object, page_index: int = 0) -> None:
        self.calls.append(("epaper", value, page_index))

    def write(self, value: object) -> None:
        self.calls.append(("printer", value))

    def set_active(self, value: object) -> None:
        self.calls.append(("audio", value))

    def set_interference(self, value: object) -> None:
        self.calls.append(("interference", value))

    def close(self) -> None:
        return None


class HardwareFrontendIntegrationTests(unittest.TestCase):
    def test_frontend_exchanges_protocol_message_and_renders_response(self) -> None:
        client_socket, backend_socket = socket.socketpair()
        observed: dict[str, object] = {}
        codec = JsonCodec()
        response = {
            "protocol_version": 1,
            "input_sequence": 1,
            "accepted": True,
            "error": None,
            "state_revision": 4,
            "output": {
                "line_lamps": [True] + [False] * 15,
                "game_phase": "ready",
                "clock": {"shift": 1, "elapsed_seconds": 65},
                "speaker_active": False,
                "interference_level": 0,
                "tap_bridge_audio_active": False,
                "tuning": {"coarse": 0, "fine": 0},
                "directory_pages": [],
                "printer_output": [],
                "call": None,
                "calls": [],
                "service_call": None,
                "tap_bridge_monitoring": None,
                "shift": {
                    "number": 1,
                    "phase": "ready",
                    "active_call_count": 0,
                    "completed_routings": 0,
                    "required_service_calls": 0,
                    "completed_service_calls": 0,
                    "service_errors": 0,
                    "service_error_counts": [],
                },
                "debug": {"messages": []},
            },
        }

        def backend() -> None:
            request = codec.decode(receive_frame(backend_socket))
            observed.update(request)
            send_frame(backend_socket, codec.encode(response))
            backend_socket.close()

        thread = threading.Thread(target=backend)
        thread.start()
        components = [Spy() for _ in range(5)]
        frontend = HardwareFrontend(
            BackendClient(client_socket, codec),
            DummyInputSource(
                [
                    PhysicalInput(
                        cord_topology=[{"first": "subscriber_0", "second": "operator"}],
                        directory_digits=[0, 0, 0, 1],
                    )
                ]
            ),
            OutputMapper(*components),
        )

        result = frontend.step()
        frontend.close()
        thread.join(timeout=1)

        self.assertTrue(result["accepted"])
        self.assertEqual(observed["input_sequence"], 1)
        self.assertEqual(observed["expected_state_revision"], 0)
        self.assertEqual(
            observed["input"]["cord_topology"],
            [{"first": "subscriber_0", "second": "operator"}],
        )
        self.assertEqual(frontend.state_revision, 4)
        self.assertEqual(components[0].calls[0], ("lines", [True] + [False] * 15))
