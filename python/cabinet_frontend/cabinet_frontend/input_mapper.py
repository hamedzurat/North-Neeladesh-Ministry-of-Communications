"""Convert physical component readings into Cabinet Frontend input state."""

from __future__ import annotations

import time
from collections.abc import Callable
from typing import Protocol

from .state import HeldControls, PhysicalInput


class Rotary(Protocol):
    def read(self) -> int: ...


class Scanner(Protocol):
    def find_pairs(self) -> list[tuple[int, int]]: ...


class Controls(Protocol):
    directory_digits: list[int]

    def poll(self) -> HeldControls: ...

    def close(self) -> None: ...


class InputSource(Protocol):
    def poll(self, now: float | None = None) -> PhysicalInput: ...

    def close(self) -> None: ...


class PhysicalInputSource:
    def __init__(
        self,
        rotary: Rotary,
        scanner: Scanner,
        pin_to_port: dict[int, str],
        directory_digits: tuple[int, int, int, int],
        pair_scan_interval: float = 2.0,
        clock_ms: Callable[[], int] | None = None,
        controls: Controls | None = None,
        crank_detents_per_rotation: int = 16,
        status_interval: float = 5.0,
        tuning: tuple[int, int] = (0, 0),
    ) -> None:
        self.rotary = rotary
        self.scanner = scanner
        self.pin_to_port = pin_to_port
        self.directory_digits = list(directory_digits)
        self.pair_scan_interval = pair_scan_interval
        self.clock_ms = clock_ms or (lambda: int(time.monotonic() * 1000))
        self.controls = controls
        if status_interval <= 0:
            raise ValueError("status_interval must be positive")
        if any(type(value) is not int or not 0 <= value <= 1023 for value in tuning):
            raise ValueError("tuning values must be integers between 0 and 1023")
        self.status_interval = status_interval
        self.tuning = tuning
        if crank_detents_per_rotation <= 0:
            raise ValueError("crank_detents_per_rotation must be positive")
        self.crank_detents_per_rotation = crank_detents_per_rotation
        self.crank_detents = 0
        self.rotation_timestamps: list[int] = []
        self.topology: list[dict[str, str]] = []
        self.held_controls = HeldControls()
        self.next_scan_at = 0.0
        self.faults: list[str] = []
        self._last_status: tuple[object, ...] | None = None
        self._next_status_log = 0.0

    def poll(self, now: float | None = None) -> PhysicalInput:
        now = time.monotonic() if now is None else now
        self.faults.clear()
        event = 0
        try:
            event = self.rotary.read()
            if event:
                print(f"ROTARY {event:+d}", flush=True)
            if event:
                self.crank_detents += 1
                if self.crank_detents >= self.crank_detents_per_rotation:
                    self.crank_detents -= self.crank_detents_per_rotation
                    timestamp = self.clock_ms()
                    self.rotation_timestamps.append(timestamp)
                    self.rotation_timestamps = self.rotation_timestamps[-4:]
                    print(f"CRANK {timestamp}", flush=True)
        except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
            self.faults.append(f"rotary: {type(error).__name__}: {error}")
        if now >= self.next_scan_at:
            try:
                self.topology = self._scan_topology()
            except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
                self.faults.append(f"pair_detector: {type(error).__name__}: {error}")
            self.next_scan_at = now + self.pair_scan_interval
        if self.controls is None:
            held_controls = HeldControls()
        else:
            try:
                held_controls = self.controls.poll()
                self.held_controls = held_controls
            except Exception as error:  # noqa: BLE001 - hardware libraries vary their errors
                self.faults.append(f"controls: {type(error).__name__}: {error}")
                held_controls = self.held_controls
        directory_digits = list(
            self.controls.directory_digits if self.controls is not None else self.directory_digits
        )
        physical = PhysicalInput(
            cord_topology=list(self.topology),
            held_controls=held_controls,
            directory_digits=directory_digits,
            crank_rotation_timestamps=list(self.rotation_timestamps),
            tuning={"coarse": self.tuning[0], "fine": self.tuning[1]},
        )
        self._log_status(physical, event, now)
        return physical

    def _log_status(self, physical: PhysicalInput, rotary_event: int, now: float) -> None:
        controls = physical.held_controls.to_wire()
        active_controls = ",".join(name for name, active in controls.items() if active) or "-"
        patch_panel = (
            ",".join(
                f"{connection['first']}>{connection['second']}"
                for connection in physical.cord_topology
            )
            or "-"
        )
        status = (
            tuple(sorted(controls.items())),
            tuple(physical.directory_digits),
            tuple(
                (connection["first"], connection["second"]) for connection in physical.cord_topology
            ),
            tuple(physical.crank_rotation_timestamps),
            tuple(self.faults),
        )
        if status == self._last_status and now < self._next_status_log:
            return
        message = (
            "INPUT // "
            f"switches={active_controls} "
            f"digits={''.join(map(str, physical.directory_digits))} "
            f"patch={patch_panel} "
            f"rotary={rotary_event:+d} "
            f"crank={physical.crank_rotation_timestamps[-1] if physical.crank_rotation_timestamps else '-'}"
        )
        if self.faults:
            message += f" faults={';'.join(self.faults)}"
        print(message, flush=True)
        self._last_status = status
        self._next_status_log = now + self.status_interval

    def _scan_topology(self) -> list[dict[str, str]]:
        cords: list[dict[str, str]] = []
        for first_pin, second_pin in self.scanner.find_pairs():
            first = self.pin_to_port.get(first_pin)
            second = self.pin_to_port.get(second_pin)
            if first is None or second is None or first == second:
                continue
            cords.append({"first": first, "second": second})
        return cords[:8]

    def close(self) -> None:
        for name, component in (("rotary", self.rotary), ("pair_detector", self.scanner)):
            try:
                component.close()
            except Exception as error:  # noqa: BLE001 - hardware cleanup must be best effort
                print(
                    f"CABINET FRONTEND // cleanup failed component={name} "
                    f"error={type(error).__name__}: {error}",
                    flush=True,
                )
        if self.controls is not None:
            try:
                self.controls.close()
            except Exception as error:  # noqa: BLE001 - hardware cleanup must be best effort
                print(
                    "CABINET FRONTEND // cleanup failed component=controls "
                    f"error={type(error).__name__}: {error}",
                    flush=True,
                )
