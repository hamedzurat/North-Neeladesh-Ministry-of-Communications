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
        self._pins = [
            *(DigitalInOut(getattr(board, f"D{pin}")) for pin in toggle_pins),
            *(DigitalInOut(getattr(board, f"D{pin}")) for pin in button_pins),
        ]
        for pin in self._pins:
            pin.switch_to_input(pull=Pull.UP)
        self._toggles = self._pins[:4]
        self._toggle_pressed = [not pin.value for pin in self._toggles]
        self._toggle_changed_at = [0.0] * 4
        self._buttons = self._pins[4:]
        self._button_pressed = [not pin.value for pin in self._buttons]
        self._button_triggered = [False] * 4
        self._button_changed_at = [0.0] * 4
        self._button_last_increment_at = [float("-inf")] * 4
        self._debounce_seconds = debounce_ms / 1000
        self._clock = clock or time.monotonic
        self._lock = threading.Lock()
        now = self._clock()
        self._toggle_changed_at = [now] * 4
        self._button_changed_at = [now] * 4
        self._stable_toggles = list(self._toggle_pressed)
        self.directory_digits = list(directory_digits)

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
            for index, pin in enumerate(self._buttons):
                pressed = not pin.value
                if pressed != self._button_pressed[index]:
                    self._button_pressed[index] = pressed
                    self._button_triggered[index] = False
                    self._button_changed_at[index] = now
                    log_runtime(
                        f"BUTTON // index={index} edge={'press' if pressed else 'release'}"
                    )
                    if (
                        pressed
                        and now - self._button_last_increment_at[index] >= self._debounce_seconds
                    ):
                        self.directory_digits[index] = (self.directory_digits[index] + 1) % 10
                        self._button_last_increment_at[index] = now
                        log_runtime(
                            f"BUTTON // index={index} incremented digits={''.join(map(str, self.directory_digits))}"
                        )
                    if pressed:
                        # This edge has been handled, even when it falls
                        # inside the debounce window after a prior press.
                        self._button_triggered[index] = True
                elif pressed and not self._button_triggered[index]:
                    # A press edge may be followed by a delayed poll while the
                    # e-paper worker holds the interpreter. Count it once the
                    # debounce interval has elapsed, if it is still held.
                    if now - self._button_changed_at[index] >= self._debounce_seconds:
                        self.directory_digits[index] = (self.directory_digits[index] + 1) % 10
                        self._button_triggered[index] = True
                        log_runtime(
                            f"BUTTON // index={index} incremented digits={''.join(map(str, self.directory_digits))}"
                        )
            return HeldControls(
                ptt=self._stable_toggles[0],
                police=self._stable_toggles[1],
                ems=self._stable_toggles[2],
                tap=self._stable_toggles[3],
            )

    def directory_digits_snapshot(self) -> list[int]:
        with self._lock:
            return list(self.directory_digits)

    def close(self) -> None:
        for pin in self._pins:
            pin.deinit()
