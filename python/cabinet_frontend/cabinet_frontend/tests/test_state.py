from __future__ import annotations

import unittest

from cabinet_frontend.state import PhysicalInput, input_message


class StateTests(unittest.TestCase):
    def test_input_message_uses_current_ring_line(self) -> None:
        message = input_message(PhysicalInput(), 1, 0, "test", [])

        self.assertEqual(message["protocol_version"], 3)
        self.assertEqual(message["input"]["ring_line"], -1)


if __name__ == "__main__":
    unittest.main()
