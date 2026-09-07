from __future__ import annotations

import unittest
from io import StringIO

from hardware_frontend.components.factory import build_dummy_components
from hardware_frontend.hardware_smoke import exercise_components


class HardwareSmokeTests(unittest.TestCase):
    def test_smoke_sequence_exercises_dummy_components_without_hardware(self) -> None:
        output = StringIO()
        components = build_dummy_components(output)
        try:
            exercise_components(components, output, sleep=lambda _: None, led_delay=0)
        finally:
            components.close()

        text = output.getvalue()
        self.assertIn("HARDWARE // WS2812 walking test", text)
        self.assertIn("DUMMY seven_segment.show('8888')", text)
        self.assertIn("DUMMY epaper.show_directory", text)
        self.assertIn("HARDWARE // pair detector found", text)
