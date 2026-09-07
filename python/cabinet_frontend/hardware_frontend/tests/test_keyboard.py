from __future__ import annotations

import unittest

from hardware_frontend.components.keyboard import KeyboardControls


class KeyboardTests(unittest.TestCase):
    def test_bindings_are_explicit_and_unknown_keys_do_nothing(self) -> None:
        self.assertEqual(KeyboardControls.KEY_BINDINGS["p"], "ptt")
        self.assertNotIn("z", KeyboardControls.KEY_BINDINGS)

    def test_directory_edit_mode_sets_selected_digits(self) -> None:
        controls = KeyboardControls(input_stream=StringInput(), output=StringOutput())
        controls.handle_key("d")
        controls.handle_key("7")

        self.assertEqual(controls.directory_digits, [0, 0, 0, 7])


class StringInput:
    def isatty(self) -> bool:
        return False


class StringOutput:
    def write(self, value: str) -> int:
        return len(value)

    def flush(self) -> None:
        return None
