from __future__ import annotations


class RotaryDecoder:
    def __init__(self, initial_state: int) -> None:
        self.last_state = initial_state

    def update(self, state: int) -> int:
        if self.last_state == 3 and state != 3:
            self.last_state = state
            return 1
        self.last_state = state
        return 0


class GpioRotaryEncoder:
    """Read a rotary encoder connected directly to Raspberry Pi GPIO."""

    def __init__(self, s1: int, s2: int) -> None:
        import board
        from digitalio import DigitalInOut, Pull

        self.s1 = DigitalInOut(getattr(board, f"D{s1}"))
        self.s2 = DigitalInOut(getattr(board, f"D{s2}"))
        self.s1.switch_to_input(pull=Pull.UP)
        self.s2.switch_to_input(pull=Pull.UP)
        self.decoder = RotaryDecoder(self._state())
        self.last_reported_state: int | None = None

    def _state(self) -> int:
        return (int(self.s1.value) << 1) | int(self.s2.value)

    def read(self) -> int:
        state = self._state()
        event = self.decoder.update(state)
        if state != self.last_reported_state or event:
            self.last_reported_state = state
        return event

    def close(self) -> None:
        self.s1.deinit()
        self.s2.deinit()
