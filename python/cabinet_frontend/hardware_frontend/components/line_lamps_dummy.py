from __future__ import annotations

from collections.abc import Sequence
from typing import TextIO


class StdoutLineLamps:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def set_lines(self, lines: Sequence[bool]) -> None:
        print(f"DUMMY line_lamps.set({list(lines)!r})", file=self.output)

    def close(self) -> None:
        print("DUMMY line_lamps.close()", file=self.output)
