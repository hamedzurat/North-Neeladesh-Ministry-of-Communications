"""Apply authoritative backend output to Cabinet Frontend components."""

from __future__ import annotations

import time
from functools import partial
from typing import Any


class OutputMapper:
    def __init__(
        self,
        line_lamps: Any,
        seven_segment: Any,
        epaper: Any,
        printer: Any,
        line_lamp_count: int = 12,
        epaper_page_interval: float = 8.0,
        clock: Any = time.monotonic,
    ) -> None:
        self.line_lamps = line_lamps
        self.seven_segment = seven_segment
        self.epaper = epaper
        self.printer = printer
        self.line_lamp_count = line_lamp_count
        self.epaper_page_interval = epaper_page_interval
        self.clock = clock
        self._last_lines: list[bool] | None = None
        self._last_seven_segment: str | None = None
        self._last_pages: list[dict[str, object]] | None = None
        self._page_index = 0
        self._next_page_at = 0.0
        self._seen_printer_entries: set[int] = set()
        self._last_run_generation: int | None = None
        self.faults: list[str] = []

    def apply(self, output: dict[str, Any], now: float | None = None) -> None:
        self.faults.clear()
        lines = [bool(value) for value in output.get("line_lamps", [False] * self.line_lamp_count)]
        lines = lines[: self.line_lamp_count]
        if lines != self._last_lines and self._try(
            "line_lamps", lambda: self.line_lamps.set_lines(lines)
        ):
            self._last_lines = lines

        clock = output.get("clock", {})
        elapsed = max(0, int(clock.get("elapsed_seconds", 0)))
        display = f"{(elapsed // 3600) % 100:02d}{(elapsed // 60) % 60:02d}"
        if display != self._last_seven_segment and self._try(
            "seven_segment", lambda: self.seven_segment.show(display)
        ):
            self._last_seven_segment = display

        pages = [dict(page) for page in output.get("directory_pages", [])]
        if pages != self._last_pages:
            self._page_index = 0
            if self._try("epaper", lambda: self.epaper.show_directory(pages, self._page_index)):
                self._last_pages = pages
            self._next_page_at = (self.clock() if now is None else now) + self.epaper_page_interval
        elif (
            pages
            and len(pages) > 1
            and (self.clock() if now is None else now) >= self._next_page_at
        ):
            self._page_index = (self._page_index + 1) % len(pages)
            self._try("epaper", lambda: self.epaper.show_directory(pages, self._page_index))
            self._next_page_at = (self.clock() if now is None else now) + self.epaper_page_interval

        run_generation = int(output.get("run_generation", 0))
        if self._last_run_generation is not None and run_generation != self._last_run_generation:
            self._seen_printer_entries.clear()
        self._last_run_generation = run_generation
        printer_entries = output.get("printer_output", [])
        for entry in printer_entries:
            entry_id = int(entry.get("entry_id", -1))
            printer_text = str(entry.get("text", ""))
            if entry_id not in self._seen_printer_entries and self._try(
                "printer", partial(self.printer.write, printer_text)
            ):
                self._seen_printer_entries.add(entry_id)

    def _try(self, name: str, operation: Any) -> bool:
        try:
            operation()
        except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
            self.faults.append(f"{name}: {type(error).__name__}: {error}")
            return False
        return True

    def close(self) -> None:
        errors: list[Exception] = []
        for component in (
            self.line_lamps,
            self.seven_segment,
            self.epaper,
            self.printer,
        ):
            try:
                component.close()
            except Exception as error:  # noqa: BLE001 - clean every output device
                errors.append(error)
        if errors:
            raise RuntimeError(f"{len(errors)} output components failed to close") from errors[0]
