from __future__ import annotations

import threading
import time
from collections.abc import Callable, Sequence

from ..diagnostics import log_runtime
from ..state import HeldControls


class GpioControls:
    """Read cabinet switches and increment directory digits from GPIO buttons."""

    def __init__(
        self,
        toggle_pins: Sequence[int],
        button_pins: Sequence[int],
        directory_digits: Sequence[int] = (0, 0, 0, 1),
        debounce_ms: int = 50,
        clock: Callable[[], float] | None = None,
    ) -> None:
        import board
        from digitalio import DigitalInOut, Pull

        if len(toggle_pins) != 4 or len(button_pins) != 4:
            raise ValueError("four toggle pins and four button pins are required")
        if len(directory_digits) != 4 or any(not 0 <= digit <= 9 for digit in directory_digits):
            raise ValueError("four directory digits between 0 and 9 are required")
        if debounce_ms < 0:
            raise ValueError("debounce_ms must be non-negative")
        self._debounce_seconds = debounce_ms / 1000
        self._clock = clock or time.monotonic
        self._lock = threading.RLock()
        self.directory_digits = list(directory_digits)
        self._button_last_increment_at = [float("-inf")] * 4
        self._pins = [DigitalInOut(getattr(board, f"D{pin}")) for pin in toggle_pins]
        for pin in self._pins:
            pin.switch_to_input(pull=Pull.UP)
        self._toggles = self._pins[:4]
        self._toggle_pressed = [not pin.value for pin in self._toggles]
        self._toggle_changed_at = [0.0] * 4
        self._buttons: list[object] = []
        self._async_buttons: list[object] = []
        try:
            from gpiozero import Button

            self._async_buttons = [
                Button(pin, pull_up=True, bounce_time=self._debounce_seconds)
                for pin in button_pins
            ]
            for index, button in enumerate(self._async_buttons):
                button.when_pressed = lambda index=index: self._increment_button(index)
        except Exception:  # noqa: BLE001 - fall back when gpiozero is unavailable
            for pin in button_pins:
                button = DigitalInOut(getattr(board, f"D{pin}"))
                button.switch_to_input(pull=Pull.UP)
                self._buttons.append(button)
        self._button_pressed = [not pin.value for pin in self._buttons]
        self._button_triggered = [False] * 4
        self._button_changed_at = [0.0] * 4
        now = self._clock()
        self._toggle_changed_at = [now] * 4
        self._button_changed_at = [now] * 4
        self._stable_toggles = list(self._toggle_pressed)

    def poll(self) -> HeldControls:
        with self._lock:
            now = self._clock()
            for index, pin in enumerate(self._toggles):
                pressed = not pin.value
                if pressed != self._toggle_pressed[index]:
                    self._toggle_pressed[index] = pressed
                    self._toggle_changed_at[index] = now
                elif (
                    pressed != self._stable_toggles[index]
                    and now - self._toggle_changed_at[index] >= self._debounce_seconds
                ):
                    self._stable_toggles[index] = pressed
            if not self._async_buttons:
                for index, pin in enumerate(self._buttons):
                    pressed = not pin.value
                    if pressed != self._button_pressed[index]:
                        self._button_pressed[index] = pressed
                        self._button_triggered[index] = False
                        self._button_changed_at[index] = now
                        log_runtime(
                            f"BUTTON // index={index} edge={'press' if pressed else 'release'}"
                        )
                        if pressed:
                            self._increment_button(index)
            return HeldControls(
                ptt=self._stable_toggles[0],
                police=self._stable_toggles[1],
                ems=self._stable_toggles[2],
                tap=self._stable_toggles[3],
            )

    def directory_digits_snapshot(self) -> list[int]:
        with self._lock:
            return list(self.directory_digits)

    def _increment_button(self, index: int) -> None:
        with self._lock:
            now = self._clock()
            if now - self._button_last_increment_at[index] < self._debounce_seconds:
                return
            self.directory_digits[index] = (self.directory_digits[index] + 1) % 10
            self._button_last_increment_at[index] = now
            log_runtime(
                f"BUTTON // index={index} incremented digits={''.join(map(str, self.directory_digits))}"
            )

    def close(self) -> None:
        for button in self._async_buttons:
            button.close()
        for pin in self._pins:
            pin.deinit()
