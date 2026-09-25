"""Apply authoritative backend output to Cabinet Frontend components."""

from __future__ import annotations

import threading
import time
from collections.abc import Callable
from functools import partial
from typing import Any

from .diagnostics import ChangeLogger


class OutputMapper:
    AUDIO_SAMPLE_RATE = 24_000
    GAME_START_MINUTES = 8 * 60

    def __init__(
        self,
        line_lamps: Any,
        seven_segment: Any,
        epaper: Any,
        printer: Any,
        line_lamp_count: int = 12,
        epaper_page_interval: float = 8.0,
        epaper_update_delay: float = 0.0,
        local_clock_display: bool = False,
        local_audio_queue: Any | None = None,
        clock: Any = time.monotonic,
        status_sink: Callable[[str], None] = print,
    ) -> None:
        self.line_lamps = line_lamps
        self.seven_segment = seven_segment
        self.epaper = epaper
        self.printer = printer
        self.line_lamp_count = line_lamp_count
        self.epaper_page_interval = epaper_page_interval
        self.epaper_update_delay = epaper_update_delay
        self.local_clock_display = local_clock_display
        self.local_audio_queue = local_audio_queue
        self.clock = clock
        self._last_lines: list[bool] | None = None
        self._last_seven_segment: str | None = None
        self._clock_start_monotonic: float | None = None
        self._clock_run_generation: int | None = None
        self._clock_lock = threading.Lock()
        self._clock_closed = False
        self._clock_thread: threading.Thread | None = None
        self._last_pages: list[dict[str, object]] | None = None
        self._page_index = 0
        self._next_page_at = 0.0
        self._seen_printer_entries: set[int] = set()
        self._last_run_generation: int | None = None
        self._last_calls: tuple[tuple[int, int, str], ...] | None = None
        self._last_service: tuple[str, str] | None = None
        self._last_local_audio: str | None = None
        self._connected_call_starts: dict[tuple[int, int], float] = {}
        self._connected_call_generation: int | None = None
        self.status_sink = status_sink or (lambda _message: None)
        self.diagnostics = ChangeLogger(self.status_sink)
        self.faults: list[str] = []
        self._epaper_condition = threading.Condition()
        self._epaper_pending: tuple[list[dict[str, object]], int] | None = None
        self._epaper_deadline: float | None = None
        self._epaper_closed = False
        self._epaper_thread: threading.Thread | None = None
        if self.epaper_update_delay > 0:
            self._epaper_thread = threading.Thread(
                target=self._epaper_loop,
                name="cabinet-epaper-worker",
                daemon=True,
            )
            self._epaper_thread.start()
        if self.local_clock_display:
            self._clock_thread = threading.Thread(
                target=self._clock_loop,
                name="cabinet-local-clock",
                daemon=True,
            )
            self._clock_thread.start()

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
        run_generation = int(output.get("run_generation", 0))
        connected_calls = {
            (int(call.get("caller_line", -1)), int(call.get("requested_callee_line", -1)))
            for call in output.get("calls", [])
            if isinstance(call, dict) and call.get("phase") == "connected"
        }
        now_monotonic = time.monotonic()
        if run_generation != self._connected_call_generation:
            self._connected_call_generation = run_generation
            self._connected_call_starts.clear()
        self._connected_call_starts = {
            key: started
            for key, started in self._connected_call_starts.items()
            if key in connected_calls
        }
        for key in connected_calls:
            self._connected_call_starts.setdefault(key, now_monotonic)
        if self.local_clock_display:
            if run_generation != self._clock_run_generation:
                self._clock_run_generation = run_generation
                self._clock_start_monotonic = time.monotonic() - elapsed
            self._update_local_clock()
        else:
            self._show_elapsed(elapsed)

        pages = [dict(page) for page in output.get("directory_pages", [])]
        if pages != self._last_pages:
            self._page_index = 0
            if self._request_epaper(pages, self._page_index):
                self._last_pages = pages
            self._next_page_at = (self.clock() if now is None else now) + self.epaper_page_interval
        elif (
            pages
            and len(pages) > 1
            and (self.clock() if now is None else now) >= self._next_page_at
        ):
            self._page_index = (self._page_index + 1) % len(pages)
            self._request_epaper(pages, self._page_index)
            self._next_page_at = (self.clock() if now is None else now) + self.epaper_page_interval

        run_generation = int(output.get("run_generation", 0))
        if self._last_run_generation is not None and run_generation != self._last_run_generation:
            self._seen_printer_entries.clear()
            self._last_calls = None
            self._last_service = None
            for key in ("calls", "service", "printer_output"):
                self.diagnostics.reset(key)
        self._last_run_generation = run_generation
        self._log_authoritative_activity(output)
        printer_entries = output.get("printer_output", [])
        for entry in printer_entries:
            entry_id = int(entry.get("entry_id", -1))
            printer_text = str(entry.get("text", ""))
            if entry_id not in self._seen_printer_entries and self._try(
                "printer", partial(self.printer.write, printer_text)
            ):
                self._seen_printer_entries.add(entry_id)

    def _log_authoritative_activity(self, output: dict[str, Any]) -> None:
        now_monotonic = time.monotonic()
        if "calls" in output or "service_call" in output:
            calls = tuple(
                (
                    int(call.get("caller_line", -1)),
                    int(call.get("requested_callee_line", -1)),
                    str(call.get("phase", "")),
                )
                for call in output.get("calls", [])
                if isinstance(call, dict)
            )
            if calls != self._last_calls:
                self._last_calls = calls
                if calls:
                    rendered = ", ".join(
                        f"LINE {caller} -> LINE {callee} ({phase})"
                        for caller, callee, phase in calls
                    )

                    self.diagnostics.emit("calls", calls, f"CALLS // {rendered}")
                else:
                    self.diagnostics.emit("calls", calls, "CALLS // none")

            service = output.get("service_call")
            service_state = None
            if isinstance(service, dict):
                service_state = (str(service.get("service", "")), str(service.get("phase", "")))
            if service_state != self._last_service:
                self._last_service = service_state
                if service_state is not None:
                    self.diagnostics.emit(
                        "service",
                        service_state,
                        f"SERVICE // {service_state[0]} {service_state[1]}",
                    )

        if "directory_pages" in output:
            pages = tuple(repr(page) for page in output["directory_pages"])
            self.diagnostics.emit("directory_pages", pages, f"DIRECTORY // pages={len(pages)}")
        if "shift" in output and isinstance(output["shift"], dict):
            shift = output["shift"]
            shift_state = (
                shift.get("number"),
                shift.get("phase"),
            )
            self.diagnostics.emit("shift", shift_state, f"SHIFT // {shift_state}")
        if "run_generation" in output:
            self.diagnostics.emit(
                "run_generation",
                output["run_generation"],
                f"RUN // generation={output['run_generation']}",
            )
        if "printer_output" in output and isinstance(output["printer_output"], list):
            printer_state = tuple(
                (entry.get("entry_id"), entry.get("text"))
                for entry in output["printer_output"]
                if isinstance(entry, dict)
            )
            self.diagnostics.emit(
                "printer_output",
                printer_state,
                f"PRINTER // entries={len(printer_state)}",
            )

        if "tap_bridge_monitoring" in output:
            monitoring = output.get("tap_bridge_monitoring")
            if isinstance(monitoring, dict):
                value = (
                    int(monitoring.get("caller_line", -1)),
                    int(monitoring.get("callee_line", -1)),
                    int(monitoring.get("caller_tap_port", -1)),
                    int(monitoring.get("callee_tap_port", -1)),
                )
                audio_clip = monitoring.get("audio_clip")
                if isinstance(audio_clip, str) and audio_clip != self._last_local_audio:
                    if self.local_audio_queue is not None:
                        call_key = (
                            int(monitoring.get("caller_line", -1)),
                            int(monitoring.get("callee_line", -1)),
                        )
                        connected_at = self._connected_call_starts.get(call_key, now_monotonic)
                        offset_samples = max(
                            0,
                            int((now_monotonic - connected_at) * self.AUDIO_SAMPLE_RATE),
                        )
                        self.local_audio_queue.put(
                            (audio_clip, offset_samples)
                        )
                    self._last_local_audio = audio_clip
                self.diagnostics.emit(
                    "tap_bridge_monitoring",
                    value,
                    "TAP BRIDGE // monitoring "
                    f"LINE {value[0]} -> LINE {value[1]} "
                    f"ports={value[2]},{value[3]}",
                )
            else:
                self._last_local_audio = None
                self.diagnostics.emit("tap_bridge_monitoring", None, "TAP BRIDGE // monitoring stopped")

        if "speaker_active" in output or "tap_bridge_audio_active" in output:
            audio_state = (
                bool(output.get("speaker_active", False)),
                bool(output.get("tap_bridge_audio_active", False)),
            )
            self.diagnostics.emit(
                "audio_state",
                audio_state,
                f"AUDIO // speaker={audio_state[0]} tap_bridge={audio_state[1]}",
            )

        for key, label in (
            ("game_phase", "GAME PHASE"),
            ("interference_level", "INTERFERENCE"),
        ):
            if key in output:
                self.diagnostics.emit(key, output[key], f"{label} // {output[key]}")

        for key in (
            "shapla_story_beat",
            "neel_story_beat",
            "dirty_work_story_beat",
            "dirty_work_completed_contacts",
            "nahid_story_beat",
            "nahid_scam_count",
        ):
            if key in output:
                self.diagnostics.emit("story_" + key, output[key], f"STORY // {key}={output[key]}")

        if "debug" in output:
            debug_messages = output.get("debug", {}).get("messages", [])
            if isinstance(debug_messages, list):
                debug_state = tuple(
                    (str(item.get("code", "unknown")), str(item.get("message", "")))
                    for item in debug_messages
                    if isinstance(item, dict)
                )
                self.diagnostics.emit(
                    "backend_debug",
                    debug_state,
                    "BACKEND DEBUG // "
                    + (
                        "; ".join(f"{code}: {message}" for code, message in debug_state)
                        or "clear"
                    ),
                )

    def _request_epaper(self, pages: list[dict[str, object]], page_index: int) -> bool:
        if self._epaper_thread is None:
            return self._try("epaper", lambda: self.epaper.show_directory(pages, page_index))
        with self._epaper_condition:
            if self._epaper_closed:
                return False
            self._epaper_pending = ([dict(page) for page in pages], page_index)
            self._epaper_deadline = time.monotonic() + self.epaper_update_delay
            self._epaper_condition.notify()
        return True

    def _show_elapsed(self, elapsed: int) -> None:
        display = f"{(elapsed // 3600) % 100:02d}{(elapsed // 60) % 60:02d}"
        with self._clock_lock:
            if display != self._last_seven_segment and self._try(
                "seven_segment", lambda: self.seven_segment.show(display)
            ):
                self._last_seven_segment = display

    def _update_local_clock(self) -> None:
        if self._clock_start_monotonic is not None:
            elapsed = max(0, int(time.monotonic() - self._clock_start_monotonic))
            virtual_minutes = self.GAME_START_MINUTES + elapsed
            display = f"{(virtual_minutes // 60) % 100:02d}{virtual_minutes % 60:02d}"
            with self._clock_lock:
                if display != self._last_seven_segment and self._try(
                    "seven_segment", lambda: self.seven_segment.show(display)
                ):
                    self._last_seven_segment = display

    def _clock_loop(self) -> None:
        while True:
            with self._epaper_condition:
                if self._clock_closed:
                    return
            self._update_local_clock()
            time.sleep(0.25)

    def _epaper_loop(self) -> None:
        while True:
            with self._epaper_condition:
                while self._epaper_pending is None and not self._epaper_closed:
                    self._epaper_condition.wait()
                if self._epaper_closed:
                    return
                assert self._epaper_deadline is not None
                remaining = self._epaper_deadline - time.monotonic()
                if remaining > 0:
                    self._epaper_condition.wait(timeout=remaining)
                    continue
                pending = self._epaper_pending
                self._epaper_pending = None
            assert pending is not None
            pages, page_index = pending
            self._try(
                "epaper",
                lambda pages=pages, page_index=page_index: self.epaper.show_directory(
                    pages, page_index
                ),
            )

    def _try(self, name: str, operation: Any) -> bool:
        try:
            operation()
        except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
            self.faults.append(f"{name}: {type(error).__name__}: {error}")
            return False
        return True

    def close(self) -> None:
        with self._epaper_condition:
            self._clock_closed = True
            self._epaper_condition.notify_all()
        if self._clock_thread is not None:
            self._clock_thread.join(timeout=2.0)
        with self._epaper_condition:
            self._epaper_closed = True
            self._epaper_condition.notify_all()
        if self._epaper_thread is not None:
            self._epaper_thread.join(timeout=2.0)
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
