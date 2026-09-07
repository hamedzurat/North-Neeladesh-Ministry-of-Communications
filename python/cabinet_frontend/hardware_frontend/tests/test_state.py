from __future__ import annotations

import unittest

from hardware_frontend.state import PhysicalInput, input_message


class StateTests(unittest.TestCase):
    def test_input_message_pads_crank_history_to_backend_array_length(self) -> None:
        message = input_message(PhysicalInput(), 1, 0, "test", [])

        self.assertEqual(message["input"]["crank_rotation_timestamps"], [0, 0, 0, 0])


if __name__ == "__main__":
    unittest.main()
