from __future__ import annotations

import time
from collections.abc import Callable

from ..state import HeldControls


class McpPttControls:
    """Read the active-low operator PTT button from one MCP23017 pin."""

    directory_digits = [0, 0, 0, 1]

    def __init__(
        self,
        mcp: object,
        pin: int = 13,
        debounce_ms: int = 80,
        clock: Callable[[], float] | None = None,
    ) -> None:
        from digitalio import Direction, Pull

        if debounce_ms < 0:
            raise ValueError("debounce_ms must be non-negative")
        self.pin = mcp.get_pin(pin)
        self.pin.direction = Direction.INPUT
        self.pin.pull = Pull.UP
        self.clock = clock or time.monotonic
        self.debounce_seconds = debounce_ms / 1000
        initial = not self.pin.value
        self._stable_ptt = initial
        self._candidate_ptt = initial
        self._candidate_since = self.clock()

    def poll(self) -> HeldControls:
        now = self.clock()
        raw_ptt = not self.pin.value
        if raw_ptt != self._candidate_ptt:
            self._candidate_ptt = raw_ptt
            self._candidate_since = now
        if raw_ptt != self._stable_ptt and now - self._candidate_since >= self.debounce_seconds:
            self._stable_ptt = raw_ptt
        return HeldControls(ptt=self._stable_ptt)

    def close(self) -> None:
        self.pin.pull = None
