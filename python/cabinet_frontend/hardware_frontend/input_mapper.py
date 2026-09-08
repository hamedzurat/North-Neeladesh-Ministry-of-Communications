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
    ) -> None:
        self.rotary = rotary
        self.scanner = scanner
        self.pin_to_port = pin_to_port
        self.directory_digits = list(directory_digits)
        self.pair_scan_interval = pair_scan_interval
        self.clock_ms = clock_ms or (lambda: int(time.monotonic() * 1000))
        self.controls = controls
        if crank_detents_per_rotation <= 0:
            raise ValueError("crank_detents_per_rotation must be positive")
        self.crank_detents_per_rotation = crank_detents_per_rotation
        self.crank_detents = 0
        self.rotation_timestamps: list[int] = []
        self.topology: list[dict[str, str]] = []
        self.next_scan_at = 0.0
        self.faults: list[str] = []

    def poll(self, now: float | None = None) -> PhysicalInput:
        now = time.monotonic() if now is None else now
        self.faults.clear()
        try:
            event = self.rotary.read()
            if event:
                print(f"ROTARY ENCODER // event={event}", flush=True)
            if event:
                self.crank_detents += 1
                if self.crank_detents >= self.crank_detents_per_rotation:
                    self.crank_detents -= self.crank_detents_per_rotation
                    timestamp = self.clock_ms()
                    self.rotation_timestamps.append(timestamp)
                    self.rotation_timestamps = self.rotation_timestamps[-4:]
                    print(
                        f"ROTARY ENCODER // crank_rotation_timestamp={timestamp}",
                        flush=True,
                    )
        except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
            self.faults.append(f"rotary: {type(error).__name__}: {error}")
        if now >= self.next_scan_at:
            try:
                self.topology = self._scan_topology()
            except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
                self.faults.append(f"pair_detector: {type(error).__name__}: {error}")
            self.next_scan_at = now + self.pair_scan_interval
        return PhysicalInput(
            cord_topology=list(self.topology),
            held_controls=self.controls.poll() if self.controls is not None else HeldControls(),
            directory_digits=list(
                self.controls.directory_digits
                if self.controls is not None
                else self.directory_digits
            ),
            crank_rotation_timestamps=list(self.rotation_timestamps),
        )

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


class DummyInputSource:
    """Deterministic source used by tests and local development."""

    def __init__(self, snapshots: list[PhysicalInput] | None = None) -> None:
        self.snapshots = snapshots or [PhysicalInput()]
        self.index = 0

    def poll(self, now: float | None = None) -> PhysicalInput:
        snapshot = self.snapshots[min(self.index, len(self.snapshots) - 1)]
        self.index += 1
        return snapshot

    def close(self) -> None:
        return None
