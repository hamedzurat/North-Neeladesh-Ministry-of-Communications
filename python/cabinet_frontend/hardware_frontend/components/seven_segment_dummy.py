from __future__ import annotations

from typing import TextIO


class StdoutSevenSegment:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def show(self, text: str) -> None:
        print(f"DUMMY seven_segment.show({text!r})", file=self.output)

    def clear(self) -> None:
        print("DUMMY seven_segment.clear()", file=self.output)

    def close(self) -> None:
        self.clear()
