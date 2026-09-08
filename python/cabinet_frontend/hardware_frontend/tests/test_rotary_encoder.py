from __future__ import annotations

import unittest

from hardware_frontend.components.rotary_encoder import RotaryDecoder


class RotaryDecoderTests(unittest.TestCase):
    def test_counts_a_detent_without_checking_direction(self) -> None:
        clockwise = RotaryDecoder(3)
        reverse = RotaryDecoder(3)
        skipped_phase = RotaryDecoder(3)

        self.assertEqual(clockwise.update(1), 1)
        self.assertEqual(clockwise.update(3), 0)
        self.assertEqual(reverse.update(2), 1)
        self.assertEqual(reverse.update(3), 0)
        self.assertEqual(skipped_phase.update(0), 1)


if __name__ == "__main__":
    unittest.main()
