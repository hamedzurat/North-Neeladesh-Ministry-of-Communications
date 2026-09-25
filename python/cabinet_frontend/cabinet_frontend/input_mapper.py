"""Convert physical component readings into Cabinet Frontend input state."""

from __future__ import annotations

import threading
import time
from typing import Protocol

from .diagnostics import log_runtime
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
        controls: Controls | None = None,
        crank_detents_per_rotation: int = 2,
        status_interval: float = 5.0,
        tuning: tuple[int, int] = (0, 0),
        background_scanning: bool = False,
        control_poll_interval: float = 0.01,
        pair_line_interval: float = 0.02,
    ) -> None:
        self.rotary = rotary
        self.scanner = scanner
        self.pin_to_port = pin_to_port
        self.directory_digits = list(directory_digits)
        self.pair_scan_interval = pair_scan_interval
        self.pair_line_interval = pair_line_interval
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
        self.ring_line = -1
        self.topology: list[dict[str, str]] = []
        self.held_controls = HeldControls()
        self.next_scan_at = 0.0
        self.faults: list[str] = []
        self._topology_rejections: list[str] = []
        self._scan_faults: list[str] = []
        self._last_status: tuple[object, ...] | None = None
        self._next_status_log = 0.0
        self._topology_lock = threading.Lock()
        self._scan_stop = threading.Event()
        self._scan_thread: threading.Thread | None = None
        self._line_pairs: dict[int, list[tuple[int, int]]] = {}
        self._line_candidate: list[dict[str, str]] | None = None
        self._line_candidate_count = 0
        self._scan_line = 0
        self._control_thread: threading.Thread | None = None
        self._rotary_thread: threading.Thread | None = None
        self._rotary_events = 0
        self._control_poll_interval = control_poll_interval
        if background_scanning:
            self._scan_thread = threading.Thread(
                target=self._scan_loop,
                name="cabinet-cord-scanner",
                daemon=True,
            )
            self._scan_thread.start()
            if self.controls is not None:
                self._control_thread = threading.Thread(
                    target=self._control_loop,
                    name="cabinet-control-reader",
                    daemon=True,
                )
                self._control_thread.start()
            self._rotary_thread = threading.Thread(
                target=self._rotary_loop,
                name="cabinet-rotary-reader",
                daemon=True,
            )
            self._rotary_thread.start()

    def poll(self, now: float | None = None) -> PhysicalInput:
        now = time.monotonic() if now is None else now
        self.faults.clear()
        event = 0
        completed_rotation = False
        rotation_armed = False
        try:
            if self._rotary_thread is None:
                event = self.rotary.read()
            else:
                with self._topology_lock:
                    event = self._rotary_events
                    self._rotary_events = 0
            if event:
                self.crank_detents += 1
                if self.crank_detents >= self.crank_detents_per_rotation:
                    self.crank_detents -= self.crank_detents_per_rotation
                    completed_rotation = True
        except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
            self.faults.append(f"rotary: {type(error).__name__}: {error}")
        if now >= self.next_scan_at and self._scan_thread is None:
            try:
                self.topology = self._scan_topology()
                self.faults.extend(self._topology_rejections)
                physical_ring_line = self._ring_generator_line()
                if completed_rotation:
                    self.ring_line = physical_ring_line
                    log_runtime(f"CRANK // ring_line={self.ring_line}")
                    rotation_armed = True
                elif physical_ring_line != self.ring_line:
                    self.ring_line = -1
            except Exception as error:  # noqa: BLE001 - hardware libraries vary their error types
                self.faults.append(f"pair_detector: {type(error).__name__}: {error}")
            self.next_scan_at = now + self.pair_scan_interval
        elif self._scan_thread is not None:
            with self._topology_lock:
                self.topology = list(self.topology)
                self.faults.extend(self._scan_faults)
        if completed_rotation and not rotation_armed:
            self.ring_line = self._ring_generator_line()
            log_runtime(f"CRANK // ring_line={self.ring_line}")
        if self.controls is None:
            held_controls = HeldControls()
        elif self._control_thread is not None:
            with self._topology_lock:
                held_controls = self.held_controls
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
            ring_line=self.ring_line,
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
            physical.ring_line,
            tuple(self.faults),
        )
        if status == self._last_status and not self.faults:
            return
        message = (
            "INPUT // "
            f"switches={active_controls} "
            f"digits={''.join(map(str, physical.directory_digits))} "
            f"patch={patch_panel} "
            f"rotary={rotary_event:+d} "
            f"ring_line={physical.ring_line}"
        )
        if self.faults:
            message += f" faults={';'.join(self.faults)}"
        log_runtime(message)
        self._last_status = status
        self._next_status_log = now + self.status_interval

    def _scan_topology(self) -> list[dict[str, str]]:
        first_topology, first_faults = self._scan_topology_once()
        second_topology, second_faults = self._scan_topology_once()
        if first_topology != second_topology or first_faults != second_faults:
            self._topology_rejections = [
                "pair_detector: unstable scan; retaining last valid topology"
            ]
            return list(self.topology)
        self._topology_rejections = first_faults
        if first_faults:
            return list(self.topology)
        return second_topology

    def _scan_loop(self) -> None:
        while not self._scan_stop.is_set():
            try:
                scan_line = getattr(self.scanner, "find_pairs_for_line", None)
                if scan_line is None:
                    topology = self._scan_topology()
                    scan_faults = list(self._topology_rejections)
                else:
                    current_line = self._scan_line
                    self._line_pairs[current_line] = scan_line(current_line)
                    self._scan_line = (current_line + 1) % 16
                    topology, scan_faults = self._topology_from_pairs(
                        [pair for pairs in self._line_pairs.values() for pair in pairs]
                    )
                    if scan_faults:
                        self._scan_line = current_line
                        topology = list(self.topology)
                        self._line_candidate = None
                        self._line_candidate_count = 0
                    elif topology == self._line_candidate:
                        self._line_candidate_count += 1
                        if self._line_candidate_count < 2:
                            topology = list(self.topology)
                    else:
                        self._line_candidate = topology
                        self._line_candidate_count = 1
                        topology = list(self.topology)
                with self._topology_lock:
                    self.topology = topology
                    self._scan_faults = scan_faults
            except Exception as error:  # noqa: BLE001 - hardware errors are reported by poll
                with self._topology_lock:
                    self._scan_faults = [
                        f"pair_detector: {type(error).__name__}: {error}"
                    ]
            interval = (
                self.pair_line_interval
                if hasattr(self.scanner, "find_pairs_for_line")
                else self.pair_scan_interval
            )
            self._scan_stop.wait(interval)

    def _control_loop(self) -> None:
        while not self._scan_stop.is_set():
            try:
                held_controls = self.controls.poll() if self.controls is not None else HeldControls()
                with self._topology_lock:
                    self.held_controls = held_controls
            except Exception as error:  # noqa: BLE001 - hardware errors are reported by poll
                with self._topology_lock:
                    self._scan_faults = [f"controls: {type(error).__name__}: {error}"]
            self._scan_stop.wait(self._control_poll_interval)

    def _rotary_loop(self) -> None:
        while not self._scan_stop.is_set():
            try:
                event = self.rotary.read()
                if event:
                    with self._topology_lock:
                        self._rotary_events += 1
            except Exception as error:  # noqa: BLE001 - hardware errors are reported by poll
                with self._topology_lock:
                    self._scan_faults = [f"rotary: {type(error).__name__}: {error}"]
            self._scan_stop.wait(self._control_poll_interval)

    def _scan_topology_once(
        self,
    ) -> tuple[list[dict[str, str]], list[str]]:
        return self._topology_from_pairs(self.scanner.find_pairs())

    def _topology_from_pairs(
        self, pairs: list[tuple[int, int]]
    ) -> tuple[list[dict[str, str]], list[str]]:
        cords: list[dict[str, str]] = []
        used_endpoints: set[str] = set()
        endpoint_pairs: dict[str, str] = {}
        repeated_endpoints: set[str] = set()
        rejected_cords = 0
        conflicting_pairs: set[str] = set()
        for first_pin, second_pin in pairs:
            first = self.pin_to_port.get(first_pin)
            second = self.pin_to_port.get(second_pin)
            if first is None or second is None or first == second:
                continue
            if first in used_endpoints or second in used_endpoints:
                if first in used_endpoints:
                    repeated_endpoints.add(first)
                    conflicting_pairs.add(endpoint_pairs[first])
                    conflicting_pairs.add(f"{first}>{second}")
                if second in used_endpoints:
                    repeated_endpoints.add(second)
                    conflicting_pairs.add(endpoint_pairs[second])
                    conflicting_pairs.add(f"{first}>{second}")
                rejected_cords += 1
                continue
            used_endpoints.update((first, second))
            pair_name = f"{first}>{second}"
            endpoint_pairs[first] = pair_name
            endpoint_pairs[second] = pair_name
            cords.append({"first": first, "second": second})
        if repeated_endpoints:
            endpoints = ",".join(sorted(repeated_endpoints))
            return cords, [
                (
                    "pair_detector: ambiguous endpoint(s) "
                    f"{endpoints}; pairs={','.join(sorted(conflicting_pairs))}; "
                    f"ignored {rejected_cords} cord(s), retaining last valid topology"
                )
            ]
        return cords[:8], []

    def _ring_generator_line(self) -> int:
        for connection in self.topology:
            first, second = connection["first"], connection["second"]
            if first == "ring_generator" and second.startswith("subscriber_"):
                return int(second.removeprefix("subscriber_"))
            if second == "ring_generator" and first.startswith("subscriber_"):
                return int(first.removeprefix("subscriber_"))
        return -1

    def close(self) -> None:
        self._scan_stop.set()
        if self._scan_thread is not None:
            self._scan_thread.join(timeout=1.0)
        if self._control_thread is not None:
            self._control_thread.join(timeout=1.0)
        if self._rotary_thread is not None:
            self._rotary_thread.join(timeout=1.0)
        for name, component in (("rotary", self.rotary), ("pair_detector", self.scanner)):
            try:
                component.close()
            except Exception as error:  # noqa: BLE001 - hardware cleanup must be best effort
                log_runtime(
                    f"FRONTEND // cleanup failed component={name} "
                    f"error={type(error).__name__}: {error}"
                )
        if self.controls is not None:
            try:
                self.controls.close()
            except Exception as error:  # noqa: BLE001 - hardware cleanup must be best effort
                log_runtime(
                    "FRONTEND // cleanup failed component=controls "
                    f"error={type(error).__name__}: {error}"
                )
