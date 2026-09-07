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
        controls = McpPttControls(mcp, pin=13)

        self.assertEqual(mcp.requested_pin, 13)
        self.assertFalse(controls.poll().ptt)

        mcp.pin.value = False
        self.assertTrue(controls.poll().ptt)

        controls.close()
        self.assertIsNone(mcp.pin.pull)


if __name__ == "__main__":
    unittest.main()
