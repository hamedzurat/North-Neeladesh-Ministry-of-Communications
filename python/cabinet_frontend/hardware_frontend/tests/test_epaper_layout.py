from __future__ import annotations

import unittest

from hardware_frontend.components.epaper import wrap_text


class EpaperLayoutTests(unittest.TestCase):
    def test_wraps_words_and_long_unbroken_text_to_screen_width(self) -> None:
        lines = wrap_text("A long directory entry", 10, len)
        long_word_lines = wrap_text("ABCDEFGHIJK", 5, len)

        self.assertEqual(lines, ["A long", "directory", "entry"])
        self.assertEqual(long_word_lines, ["ABCDE", "FGHIJ", "K"])
