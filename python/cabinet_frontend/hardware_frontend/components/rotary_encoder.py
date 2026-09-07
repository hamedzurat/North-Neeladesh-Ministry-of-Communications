from __future__ import annotations


class McpRotaryEncoder:
    def __init__(self, mcp: object, s1: int = 11, s2: int = 12) -> None:
        from rotary_encoder import RotaryEncoder

        self.device = RotaryEncoder(mcp, s1=s1, s2=s2)

    def read(self) -> int:
        return int(self.device.read())

    def close(self) -> None:
        return None
