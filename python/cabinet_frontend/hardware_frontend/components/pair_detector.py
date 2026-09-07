from __future__ import annotations

import time


class McpPairDetector:
    def __init__(self, mcp: object, count: int = 8) -> None:
        from digitalio import Direction, Pull

        self._direction = Direction
        self._pull = Pull
        self.pins = [mcp.get_pin(index) for index in range(count)]

    def _all_inputs(self) -> None:
        for pin in self.pins:
            pin.direction = self._direction.INPUT
            pin.pull = self._pull.UP

    def find_pairs(self) -> list[tuple[int, int]]:
        pairs: list[tuple[int, int]] = []
        for index, pin in enumerate(self.pins):
            self._all_inputs()
            pin.pull = None
            pin.direction = self._direction.OUTPUT
            pin.value = False
            time.sleep(0.001)
            for other in range(index + 1, len(self.pins)):
                if not self.pins[other].value:
                    pairs.append((index, other))
        self._all_inputs()
        return pairs

    def close(self) -> None:
        self._all_inputs()
