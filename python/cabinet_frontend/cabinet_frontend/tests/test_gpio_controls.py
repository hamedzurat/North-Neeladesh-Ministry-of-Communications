from __future__ import annotations

import sys
import unittest
from types import SimpleNamespace
from unittest.mock import patch


class FakePin:
    def __init__(self, value: bool = True) -> None:
        self.value = value

    def switch_to_input(self, *, pull: object) -> None:
        return None

    def deinit(self) -> None:
        return None


class GpioControlsTests(unittest.TestCase):
    def test_button_press_increments_its_digit_once_and_snapshot_is_copy(self) -> None:
        pins = [FakePin() for _ in range(8)]
        board = SimpleNamespace(**{f"D{index}": object() for index in (1, 2, 3, 4)})
        digitalio = SimpleNamespace(
            DigitalInOut=lambda pin: pins.pop(0),
            Pull=SimpleNamespace(UP=object()),
        )
        clock = iter((0.0, 0.0, 0.06, 0.07, 0.12))
        with patch.dict(sys.modules, {"board": board, "digitalio": digitalio}):
            from cabinet_frontend.components.gpio_controls import GpioControls

            controls = GpioControls((1, 2, 3, 4), (1, 2, 3, 4), clock=lambda: next(clock))
            buttons = controls._buttons
            buttons[0].value = False
            controls.poll()
            controls.poll()
            self.assertEqual(controls.directory_digits_snapshot(), [1, 0, 0, 1])
            controls.poll()
            self.assertEqual(controls.directory_digits_snapshot(), [1, 0, 0, 1])
            buttons[0].value = True
            controls.poll()
            snapshot = controls.directory_digits_snapshot()
            snapshot[0] = 9
            self.assertEqual(controls.directory_digits_snapshot(), [1, 0, 0, 1])
            controls.close()


if __name__ == "__main__":
    unittest.main()
