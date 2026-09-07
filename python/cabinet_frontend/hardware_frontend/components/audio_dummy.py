from __future__ import annotations

from typing import TextIO


class StdoutAudio:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def set_active(self, active: bool) -> None:
        print(f"DUMMY audio.set_active({active!r})", file=self.output)

    def set_interference(self, level: int) -> None:
        print(f"DUMMY audio.set_interference({level})", file=self.output)

    def close(self) -> None:
        print("DUMMY audio.close()", file=self.output)
