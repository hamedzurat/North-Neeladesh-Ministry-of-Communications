from __future__ import annotations

from collections.abc import Sequence
from typing import TextIO


class StdoutEpaper:
    def __init__(self, output: TextIO) -> None:
        self.output = output

    def show_directory(self, pages: Sequence[dict[str, object]], page_index: int = 0) -> None:
        print(
            f"DUMMY epaper.show_directory(page={page_index}, pages={list(pages)!r})",
            file=self.output,
        )

    def close(self) -> None:
        print("DUMMY epaper.close()", file=self.output)
