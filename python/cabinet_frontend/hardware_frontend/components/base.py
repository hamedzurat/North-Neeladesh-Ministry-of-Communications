"""Small component seams shared by real and dummy hardware."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Protocol


class LineLampBank(Protocol):
    def set_lines(self, lines: Sequence[bool]) -> None: ...

    def close(self) -> None: ...


class SevenSegmentDisplay(Protocol):
    def show(self, text: str) -> None: ...

    def clear(self) -> None: ...

    def close(self) -> None: ...


class EpaperDisplay(Protocol):
    def show_directory(self, pages: Sequence[dict[str, object]], page_index: int = 0) -> None: ...

    def close(self) -> None: ...


class RotaryInput(Protocol):
    def read(self) -> int: ...

    def close(self) -> None: ...


class TopologyScanner(Protocol):
    def find_pairs(self) -> list[tuple[int, int]]: ...

    def close(self) -> None: ...


class Printer(Protocol):
    def write(self, text: str) -> None: ...

    def close(self) -> None: ...


class AudioOutput(Protocol):
    def set_active(self, active: bool) -> None: ...

    def close(self) -> None: ...
