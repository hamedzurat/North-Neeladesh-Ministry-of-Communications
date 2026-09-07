from __future__ import annotations

from ..state import HeldControls


class McpPttControls:
    """Read the active-low operator PTT button from one MCP23017 pin."""

    directory_digits = [0, 0, 0, 1]

    def __init__(self, mcp: object, pin: int = 13) -> None:
        from digitalio import Direction, Pull

        self.pin = mcp.get_pin(pin)
        self.pin.direction = Direction.INPUT
        self.pin.pull = Pull.UP

    def poll(self) -> HeldControls:
        return HeldControls(ptt=not self.pin.value)

    def close(self) -> None:
        self.pin.pull = None
