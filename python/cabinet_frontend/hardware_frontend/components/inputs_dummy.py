from __future__ import annotations

from typing import TextIO


class StdoutRotary:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def read(self) -> int:
        return 0

    def close(self) -> None:
        print("DUMMY rotary.close()", file=self.output)


class StdoutTopologyScanner:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def find_pairs(self) -> list[tuple[int, int]]:
        return []

    def close(self) -> None:
        print("DUMMY pair_detector.close()", file=self.output)
