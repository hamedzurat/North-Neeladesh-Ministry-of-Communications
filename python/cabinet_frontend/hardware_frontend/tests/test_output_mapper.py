from __future__ import annotations

import unittest

from hardware_frontend.output_mapper import OutputMapper


class Spy:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    def set_lines(self, value: object) -> None:
        self.calls.append(("lines", value))

    def show(self, value: object) -> None:
        self.calls.append(("seven_segment", value))

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


class OutputMapperTests(unittest.TestCase):
    def test_applies_state_and_does_not_repeat_static_output(self) -> None:
        components = [Spy() for _ in range(5)]
        mapper = OutputMapper(*components)
        output = {
            "line_lamps": [True, False],
            "clock": {"elapsed_seconds": 28865},
            "directory_pages": [{"page_number": 1, "heading": "A", "lines": ["B"]}],
            "printer_output": [{"entry_id": 7, "text": "ROUTING 0 -> 1"}],
            "speaker_active": True,
            "interference_level": 25,
            "tap_bridge_audio_active": False,
        }

        mapper.apply(output)
        mapper.apply(output)

        self.assertEqual(components[0].calls, [("lines", [True, False])])
        self.assertEqual(components[1].calls, [("seven_segment", "0801")])
        self.assertEqual(components[2].calls, [("epaper", output["directory_pages"], 0)])
        self.assertEqual(components[3].calls, [("printer", "ROUTING 0 -> 1")])
        self.assertEqual(components[4].calls, [("interference", 25), ("audio", True)])

    def test_cycles_directory_pages_after_interval(self) -> None:
        components = [Spy() for _ in range(5)]
        mapper = OutputMapper(*components, epaper_page_interval=8.0)
        output = {
            "line_lamps": [],
            "clock": {"elapsed_seconds": 0},
            "directory_pages": [
                {"page_number": 1, "heading": "ONE", "lines": []},
                {"page_number": 2, "heading": "TWO", "lines": []},
            ],
            "printer_output": [],
            "speaker_active": False,
        }

        mapper.apply(output, now=0)
        mapper.apply(output, now=7.9)
        mapper.apply(output, now=8.0)

        self.assertEqual(components[2].calls[0][1:], (output["directory_pages"], 0))
        self.assertEqual(components[2].calls[1][1:], (output["directory_pages"], 1))
