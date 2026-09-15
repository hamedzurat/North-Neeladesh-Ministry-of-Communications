from __future__ import annotations

import unittest
from contextlib import redirect_stdout
from io import StringIO

from cabinet_frontend.config import HardwareConfig
from cabinet_frontend.input_mapper import PhysicalInputSource
from cabinet_frontend.state import HeldControls


class Rotary:
    def __init__(self, values: list[int]) -> None:
        self.values = iter(values)

    def read(self) -> int:
        return next(self.values, 0)

    def close(self) -> None:
        return None


class Scanner:
    def __init__(self, pairs: list[tuple[int, int]]) -> None:
        self.pairs = pairs

    def find_pairs(self) -> list[tuple[int, int]]:
        return self.pairs

    def close(self) -> None:
        return None


class FailingRotary:
    def read(self) -> int:
        raise OSError("encoder unavailable")

    def close(self) -> None:
        return None


class CloseFailingRotary:
    def read(self) -> int:
        return 0

    def close(self) -> None:
        raise OSError("MCP23017 unavailable")


class TrackingScanner(Scanner):
    def __init__(self) -> None:
        super().__init__([])
        self.closed = False

    def close(self) -> None:
        self.closed = True


class Controls:
    def __init__(self) -> None:
        self.directory_digits = [1, 2, 3, 4]

    def poll(self) -> HeldControls:
        return HeldControls(ptt=True, ems=True)

    def close(self) -> None:
        return None


class InputMapperTests(unittest.TestCase):
    def test_default_mcp_ports_cover_patch_panel_and_tap_bridge(self) -> None:
        config = HardwareConfig()

        self.assertEqual(
            config.pin_to_port,
            {
                0: "subscriber_0",
                1: "subscriber_1",
                2: "subscriber_2",
                3: "subscriber_3",
                4: "subscriber_4",
                5: "subscriber_5",
                6: "subscriber_6",
                7: "subscriber_7",
                8: "subscriber_8",
                9: "subscriber_9",
                10: "subscriber_10",
                11: "subscriber_11",
                12: "operator",
                13: "ring_generator",
                14: "tap_1",
                15: "tap_2",
            },
        )

    def test_maps_pairs_and_records_completed_rotations(self) -> None:
        source = PhysicalInputSource(
            Rotary([1, 0]),
            Scanner([(0, 1), (2, 9)]),
            {0: "subscriber_0", 1: "operator", 2: "subscriber_2"},
            (0, 0, 0, 1),
            pair_scan_interval=10,
            clock_ms=iter([1234]).__next__,
            crank_detents_per_rotation=1,
        )

        first = source.poll(now=0)
        second = source.poll(now=1)

        self.assertEqual(first.cord_topology, [{"first": "subscriber_0", "second": "operator"}])
        self.assertEqual(first.crank_rotation_timestamps, [1234])
        self.assertEqual(second.cord_topology, first.cord_topology)
        self.assertEqual(second.crank_rotation_timestamps, [1234])

    def test_limits_rotation_history_to_four_timestamps(self) -> None:
        source = PhysicalInputSource(
            Rotary([1, 1, 1, 1, 1]),
            Scanner([]),
            {},
            (0, 0, 0, 1),
            clock_ms=iter([1, 2, 3, 4, 5]).__next__,
            crank_detents_per_rotation=1,
        )

        for index in range(5):
            snapshot = source.poll(now=float(index))

        self.assertEqual(snapshot.crank_rotation_timestamps, [2, 3, 4, 5])

    def test_sixteen_encoder_detents_emit_one_crank_rotation(self) -> None:
        source = PhysicalInputSource(
            Rotary([1] * 16),
            Scanner([]),
            {},
            (0, 0, 0, 1),
            clock_ms=iter([1600]).__next__,
            crank_detents_per_rotation=16,
        )

        for index in range(16):
            snapshot = source.poll(now=float(index))

        self.assertEqual(snapshot.crank_rotation_timestamps, [1600])

    def test_sixteen_encoder_detents_emit_one_crank_rotation_in_reverse(self) -> None:
        source = PhysicalInputSource(
            Rotary([-1] * 16),
            Scanner([]),
            {},
            (0, 0, 0, 1),
            clock_ms=iter([1600]).__next__,
            crank_detents_per_rotation=16,
        )

        for index in range(16):
            snapshot = source.poll(now=float(index))

        self.assertEqual(snapshot.crank_rotation_timestamps, [1600])

    def test_input_faults_are_retained_without_losing_last_topology(self) -> None:
        source = PhysicalInputSource(
            FailingRotary(),
            Scanner([(0, 1)]),
            {0: "subscriber_0", 1: "subscriber_1"},
            (0, 0, 0, 1),
        )

        snapshot = source.poll(now=0)

        self.assertEqual(
            snapshot.cord_topology, [{"first": "subscriber_0", "second": "subscriber_1"}]
        )
        self.assertEqual(source.faults, ["rotary: OSError: encoder unavailable"])

    def test_cleanup_continues_when_hardware_component_is_unavailable(self) -> None:
        scanner = TrackingScanner()
        source = PhysicalInputSource(CloseFailingRotary(), scanner, {}, (0, 0, 0, 1))

        source.close()

        self.assertTrue(scanner.closed)

    def test_logs_a_complete_input_status_on_change(self) -> None:
        output = StringIO()
        source = PhysicalInputSource(
            Rotary([0, 0]),
            Scanner([(0, 12), (14, 15)]),
            HardwareConfig().pin_to_port,
            (0, 0, 0, 1),
            controls=Controls(),
            pair_scan_interval=0,
            status_interval=5,
        )

        with redirect_stdout(output):
            source.poll(now=0)
            source.poll(now=1)

        text = output.getvalue()
        self.assertIn("INPUT //", text)
        self.assertIn("switches=ptt,ems", text)
        self.assertIn("digits=1234", text)
        self.assertIn("subscriber_0>operator", text)
        self.assertIn("tap_1>tap_2", text)
        self.assertEqual(text.count("INPUT //"), 1)
