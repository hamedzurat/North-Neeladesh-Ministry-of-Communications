from __future__ import annotations

import unittest
from io import StringIO

from hardware_frontend.components.factory import build_dummy_components


class DummyComponentsTests(unittest.TestCase):
    def test_dummy_components_report_actions_to_stdout(self) -> None:
        output = StringIO()
        components = build_dummy_components(output)
        components.line_lamps.set_lines([True, False])
        components.seven_segment.show("0105")
        components.epaper.show_directory([])
        components.printer.write("hello")
        components.audio.set_active(False)

        self.assertIn("DUMMY line_lamps.set", output.getvalue())
        self.assertIn("DUMMY seven_segment.show('0105')", output.getvalue())
        self.assertIn("DUMMY epaper.show_directory", output.getvalue())
        self.assertIn("DUMMY printer.write('hello')", output.getvalue())
