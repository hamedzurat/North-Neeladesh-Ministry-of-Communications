from __future__ import annotations

import unittest

from hardware_frontend.components.mcp_controls import McpPttControls


class Pin:
    def __init__(self) -> None:
        self.value = True
        self.direction = None
        self.pull = None


class Mcp:
    def __init__(self) -> None:
        self.pin = Pin()
        self.requested_pin = None

    def get_pin(self, pin: int) -> Pin:
        self.requested_pin = pin
        return self.pin


class McpControlsTests(unittest.TestCase):
    def test_pin_13_is_active_low_ptt(self) -> None:
        mcp = Mcp()
        controls = McpPttControls(mcp, pin=13, debounce_ms=0)

        self.assertEqual(mcp.requested_pin, 13)
        self.assertFalse(controls.poll().ptt)

        mcp.pin.value = False
        self.assertTrue(controls.poll().ptt)

        controls.close()
        self.assertIsNone(mcp.pin.pull)

    def test_ptt_requires_a_stable_level_before_changing(self) -> None:
        mcp = Mcp()
        clock_values = iter([0.0, 0.01, 0.03, 0.05, 0.06, 0.11, 0.12, 0.16, 0.18])
        controls = McpPttControls(
            mcp,
            debounce_ms=50,
            clock=clock_values.__next__,
        )

        self.assertFalse(controls.poll().ptt)
        mcp.pin.value = False
        self.assertFalse(controls.poll().ptt)
        mcp.pin.value = True
        self.assertFalse(controls.poll().ptt)
        mcp.pin.value = False
        self.assertFalse(controls.poll().ptt)
        self.assertTrue(controls.poll().ptt)
        mcp.pin.value = True
        self.assertTrue(controls.poll().ptt)
        self.assertTrue(controls.poll().ptt)
        self.assertFalse(controls.poll().ptt)


if __name__ == "__main__":
    unittest.main()
