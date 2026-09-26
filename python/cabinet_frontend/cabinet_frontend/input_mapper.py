"""Convert physical component readings into Cabinet Frontend input state."""

from __future__ import annotations

import threading
import time
from collections.abc import Callable
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
        topology_confirmation_scans: int = 1,
        empty_topology_confirmation_scans: int = 3,
        topology_stale_timeout: float = 1.0,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self.rotary = rotary
        self.scanner = scanner
        self.pin_to_port = pin_to_port
        self.directory_digits = list(directory_digits)
        self.pair_scan_interval = pair_scan_interval
        if topology_confirmation_scans <= 0:
            raise ValueError("topology_confirmation_scans must be positive")
        self.topology_confirmation_scans = topology_confirmation_scans
        if empty_topology_confirmation_scans <= 0:
            raise ValueError("empty_topology_confirmation_scans must be positive")
        self.empty_topology_confirmation_scans = empty_topology_confirmation_scans
        if topology_stale_timeout <= 0:
            raise ValueError("topology_stale_timeout must be positive")
        self.topology_stale_timeout = topology_stale_timeout
        self._clock = clock
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
        self._topology_candidate: list[dict[str, str]] | None = None
        self._topology_candidate_count = 0
        self.topology_revision = 0
        self.topology_stale = False
        self._last_valid_scan_at: float | None = None
        self.topology_status = "unknown"
        self.topology_fault: str | None = None
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
                self._set_topology(self._scan_topology())
                self.faults.extend(self._topology_rejections)
                self._refresh_stale(now)
                physical_ring_line = self._ring_generator_line()
                if completed_rotation:
                    self.ring_line = physical_ring_line
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
                self._refresh_stale(now)
        if completed_rotation and not rotation_armed:
            self.ring_line = self._ring_generator_line()
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
        if self.controls is None:
            directory_digits = list(self.directory_digits)
        else:
            snapshot = getattr(self.controls, "directory_digits_snapshot", None)
            directory_digits = list(snapshot() if callable(snapshot) else self.controls.directory_digits)
        physical = PhysicalInput(
            cord_topology=list(self.topology),
            topology_revision=self.topology_revision,
            topology_status=self.topology_status,
            topology_age_ms=(
                -1
                if self._last_valid_scan_at is None
                else max(0, int((now - self._last_valid_scan_at) * 1000))
            ),
            held_controls=held_controls,
            directory_digits=directory_digits,
            ring_line=self.ring_line,
            crank_active=event != 0,
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
            tuple(self._fault_identity(fault) for fault in self.faults),
        )
        if status == self._last_status:
            return
        message = (
            "INPUT // "
            f"switches={active_controls} "
            f"digits={''.join(map(str, physical.directory_digits))} "
            f"patch={patch_panel} "
            f"rotary={rotary_event:+d}"
        )
        if self.faults:
            message += f" faults={';'.join(self.faults)}"
        log_runtime(message)
        self._last_status = status
        self._next_status_log = now + self.status_interval

    @staticmethod
    def _fault_identity(fault: str) -> str:
        if fault.startswith("pair_detector: topology stale for "):
            return "pair_detector: topology stale"
        return fault

    @staticmethod
    def _topology_label(topology: list[dict[str, str]]) -> str:
        return ",".join(
            f"{cord['first']}>{cord['second']}" for cord in topology
        ) or "-"

    def _scan_topology(self) -> list[dict[str, str]]:
        first_topology, first_faults = self._scan_topology_once()
        second_topology, second_faults = self._scan_topology_once()
        if first_topology != second_topology or first_faults != second_faults:
            self._reset_candidate()
            self.topology_status = "unstable"
            self._topology_rejections = [
                (
                    "pair_detector: unstable scan; "
                    f"first={self._topology_label(first_topology)} "
                    f"second={self._topology_label(second_topology)}; "
                    "retaining last valid topology"
                )
            ]
            return list(self.topology)
        self._topology_rejections = first_faults
        if first_faults:
            self._reset_candidate()
            self.topology_status = "ambiguous"
            return list(self.topology)
        self._last_valid_scan_at = self._clock()
        required_scans = self.topology_confirmation_scans
        if not second_topology and self.topology:
            required_scans = max(required_scans, self.empty_topology_confirmation_scans)
        if second_topology == self._topology_candidate:
            self._topology_candidate_count += 1
        else:
            self._topology_candidate = second_topology
            self._topology_candidate_count = 1
        if self._topology_candidate_count < required_scans:
            self._topology_rejections = [
                "pair_detector: topology not yet stable; retaining last valid topology"
            ]
            return list(self.topology)
        self.topology_status = "valid" if second_topology else "empty"
        self.topology_stale = False
        return second_topology

    def _scan_loop(self) -> None:
        while not self._scan_stop.is_set():
            try:
                with self._topology_lock:
                    topology = self._scan_topology()
                    scan_faults = list(self._topology_rejections)
                    self._set_topology(topology)
                    self._scan_faults = scan_faults
            except Exception as error:  # noqa: BLE001 - hardware errors are reported by poll
                with self._topology_lock:
                    self._scan_faults = [f"pair_detector: {type(error).__name__}: {error}"]
            self._scan_stop.wait(self.pair_scan_interval)

    def _control_loop(self) -> None:
        while not self._scan_stop.is_set():
            try:
                held_controls = (
                    self.controls.poll() if self.controls is not None else HeldControls()
                )
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
        scan = getattr(self.scanner, "scan", None)
        if callable(scan):
            result = scan()
            status = getattr(result, "status", "valid")
            pairs = getattr(result, "pairs", [])
            self.topology_status = status
            self.topology_fault = getattr(result, "fault", None)
            if status not in {"valid", "empty"}:
                fault = getattr(result, "fault", None) or status
                return [], [f"pair_detector: scan {status}: {fault}"]
        else:
            pairs = self.scanner.find_pairs()
            self.topology_status = "valid" if pairs else "empty"
            self.topology_fault = None
        return self._topology_from_pairs(pairs)

    def _set_topology(self, topology: list[dict[str, str]]) -> None:
        if topology != self.topology:
            self.topology_revision += 1
        self.topology = list(topology)

    def _reset_candidate(self) -> None:
        self._topology_candidate = None
        self._topology_candidate_count = 0

    def _refresh_stale(self, now: float) -> None:
        if self._last_valid_scan_at is None:
            return
        self.topology_stale = now - self._last_valid_scan_at > self.topology_stale_timeout
        if self.topology_stale:
            self._reset_candidate()
            self.topology_status = "stale"
            self.faults.append(
                f"pair_detector: topology stale for {now - self._last_valid_scan_at:.2f}s"
            )

    def _topology_from_pairs(
        self, pairs: list[tuple[int, int]] | tuple[tuple[int, int], ...]
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
            if first is None or second is None:
                invalid_pin = first_pin if first is None else second_pin
                return [], [
                    (
                        f"pair_detector: invalid endpoint pin {invalid_pin}; "
                        "retaining last valid topology"
                    )
                ]
            if first == second:
                return [], [
                    f"pair_detector: self-connection {first}; retaining last valid topology"
                ]
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
        if len(cords) > 8:
            return [], [
                (
                    f"pair_detector: found {len(cords)} cords, maximum is 8; "
                    "retaining last valid topology"
                )
            ]
        return cords, []

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
