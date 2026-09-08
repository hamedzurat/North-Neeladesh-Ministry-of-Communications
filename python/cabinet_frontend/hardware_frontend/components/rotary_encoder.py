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


class McpRotaryEncoder:
    def __init__(self, mcp: object, s1: int = 11, s2: int = 12) -> None:
        from digitalio import Pull

        self.s1 = mcp.get_pin(s1)
        self.s2 = mcp.get_pin(s2)
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
            print(f"ROTARY ENCODER // pins={state:02b} event={event}", flush=True)
            self.last_reported_state = state
        return event

    def close(self) -> None:
        self.s1.pull = None
        self.s2.pull = None
