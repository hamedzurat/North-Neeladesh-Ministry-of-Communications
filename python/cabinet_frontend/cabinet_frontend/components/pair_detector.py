from __future__ import annotations

import time
from dataclasses import dataclass
from threading import Lock


@dataclass(frozen=True)
class PairScanResult:
    pairs: tuple[tuple[int, int], ...]
    status: str
    scan_id: int
    started_at: float
    finished_at: float
    fault: str | None = None

    @property
    def duration(self) -> float:
        return self.finished_at - self.started_at


class McpPairDetector:
    def __init__(
        self,
        mcp: object,
        pins: list[int] | tuple[int, ...] = tuple(range(16)),
        probe_settle_time: float = 0.001,
    ) -> None:
        if probe_settle_time < 0:
            raise ValueError("probe_settle_time must be non-negative")
        self._mcp = mcp
        self.pins = [mcp.get_pin(index) for index in pins]
        self.pin_numbers = list(pins)
        self._pin_mask = sum(1 << pin for pin in self.pin_numbers)
        self.probe_settle_time = probe_settle_time
        self._scan_lock = Lock()
        self._scan_id = 0

    def _all_inputs(self) -> None:
        self._mcp.iodir = self._pin_mask
        self._mcp.gppu = self._pin_mask

    def scan(self) -> PairScanResult:
        with self._scan_lock:
            self._scan_id += 1
            scan_id = self._scan_id
            started_at = time.monotonic()
            pairs: list[tuple[int, int]] = []
            try:
                samples: list[tuple[int, int]] = []
                for index, pin in enumerate(self.pins):
                    self._all_inputs()
                    probe_bit = 1 << self.pin_numbers[index]
                    self._mcp.gpio = self._pin_mask & ~probe_bit
                    self._mcp.gppu = self._pin_mask & ~probe_bit
                    self._mcp.iodir = self._pin_mask & ~probe_bit
                    time.sleep(self.probe_settle_time)
                    samples.append((index, self._mcp.gpio))
                for index, gpio in samples:
                    probe_pin = self.pin_numbers[index]
                    for other in range(index + 1, len(self.pins)):
                        other_pin = self.pin_numbers[other]
                        other_gpio = samples[other][1]
                        if not (gpio & (1 << other_pin)) and not (other_gpio & (1 << probe_pin)):
                            pairs.append((probe_pin, other_pin))
                status = "valid" if pairs else "empty"
                fault = None
            except Exception as error:  # noqa: BLE001 - hardware libraries vary
                status = "hardware_error"
                fault = f"{type(error).__name__}: {error}"
                pairs = []
            finally:
                try:
                    self._all_inputs()
                except Exception as error:  # noqa: BLE001 - cleanup is part of scan safety
                    status = "hardware_error"
                    cleanup_fault = f"cleanup {type(error).__name__}: {error}"
                    fault = f"{fault}; {cleanup_fault}" if fault else cleanup_fault
                    pairs = []
            finished_at = time.monotonic()
            return PairScanResult(tuple(pairs), status, scan_id, started_at, finished_at, fault)

    def find_pairs(self) -> list[tuple[int, int]]:
        """Return pairs for legacy callers and the hardware smoke test."""
        result = self.scan()
        return list(result.pairs)

    def close(self) -> None:
        with self._scan_lock:
            self._all_inputs()
