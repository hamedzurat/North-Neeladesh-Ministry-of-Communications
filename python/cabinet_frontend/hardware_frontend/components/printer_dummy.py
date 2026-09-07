from __future__ import annotations

from typing import TextIO


class StdoutPrinter:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def write(self, text: str) -> None:
        print(f"DUMMY printer.write({text!r})", file=self.output)

    def close(self) -> None:
        print("DUMMY printer.close()", file=self.output)
