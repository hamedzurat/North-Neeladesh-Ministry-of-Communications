from __future__ import annotations

import sys
import types
import unittest
from unittest.mock import patch


class Direction:
    INPUT = "input"
    OUTPUT = "output"


class Pull:
    UP = "up"


class Pin:
    def __init__(self, index: int, pins: list[Pin]) -> None:
        self.index = index
        self.pins = pins
        self.direction = Direction.INPUT
        self.pull = Pull.UP
        self._value = True

    @property
    def value(self) -> bool:
        if self.direction == Direction.OUTPUT:
            return self._value
        return not any(
            other.direction == Direction.OUTPUT
            and not other._value
            and other.index in {0, 2}
            and self.index in {0, 2}
            for other in self.pins
        )

    @value.setter
    def value(self, value: bool) -> None:
        self._value = value


class Mcp:
    def __init__(self) -> None:
        self.pins: list[Pin] = []
        self.gpio_reads = 0
        self._iodir = 0xFFFF
        self._gppu = 0xFFFF
        self.pins.extend(Pin(index, self.pins) for index in range(4))

    def get_pin(self, index: int) -> Pin:
        return self.pins[index]

    @property
    def iodir(self) -> int:
        return self._iodir

    @iodir.setter
    def iodir(self, value: int) -> None:
        self._iodir = value
        for index, pin in enumerate(self.pins):
            pin.direction = Direction.INPUT if value & (1 << index) else Direction.OUTPUT

    @property
    def gppu(self) -> int:
        return self._gppu

    @gppu.setter
    def gppu(self, value: int) -> None:
        self._gppu = value
        for index, pin in enumerate(self.pins):
            pin.pull = Pull.UP if value & (1 << index) else None

    @property
    def gpio(self) -> int:
        self.gpio_reads += 1
        return sum(1 << index for index, pin in enumerate(self.pins) if pin.value)

    @gpio.setter
    def gpio(self, value: int) -> None:
        for index, pin in enumerate(self.pins):
            pin._value = bool(value & (1 << index))


class FailingPin(Pin):
    @property
    def value(self) -> bool:
        raise OSError("read failed")


class FailingMcp(Mcp):
    def __init__(self) -> None:
        super().__init__()
        self.pins[1] = FailingPin(1, self.pins)


class PairDetectorTests(unittest.TestCase):
    def test_scan_finds_connected_pair_and_restores_inputs(self) -> None:
        digitalio = types.SimpleNamespace(Direction=Direction, Pull=Pull)
        with patch.dict(sys.modules, {"digitalio": digitalio}):
            from cabinet_frontend.components.pair_detector import McpPairDetector

            mcp = Mcp()
            detector = McpPairDetector(mcp, (0, 1, 2, 3), probe_settle_time=0)
            result = detector.scan()

        self.assertEqual(result.status, "valid")
        self.assertEqual(result.pairs, ((0, 2),))
        self.assertEqual(mcp.gpio_reads, 4)
        self.assertTrue(all(pin.direction == Direction.INPUT for pin in mcp.pins))
        self.assertTrue(all(pin.pull == Pull.UP for pin in mcp.pins))

    def test_legacy_find_pairs_returns_empty_after_scan_failure(self) -> None:
        digitalio = types.SimpleNamespace(Direction=Direction, Pull=Pull)
        with patch.dict(sys.modules, {"digitalio": digitalio}):
            from cabinet_frontend.components.pair_detector import McpPairDetector

            mcp = FailingMcp()
            detector = McpPairDetector(mcp, (0, 1), probe_settle_time=0)

            self.assertEqual(detector.find_pairs(), [])
