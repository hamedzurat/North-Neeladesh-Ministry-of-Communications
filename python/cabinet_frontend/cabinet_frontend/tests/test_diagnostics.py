from __future__ import annotations

import unittest

from cabinet_frontend.diagnostics import ChangeLogger


class ChangeLoggerTests(unittest.TestCase):
    def test_emits_first_value_and_changes_but_not_repeated_values(self) -> None:
        messages: list[str] = []
        logger = ChangeLogger(messages.append)

        logger.emit("transport", "connected", "connected")
        logger.emit("transport", "connected", "connected")
        logger.emit("transport", "offline", "offline")
        logger.emit("transport", "offline", "offline")

        self.assertEqual(messages, ["connected", "offline"])

    def test_reset_allows_a_recovery_message_to_be_emitted_again(self) -> None:
        messages: list[str] = []
        logger = ChangeLogger(messages.append)

        logger.emit("transport", "connected", "connected")
        logger.reset("transport")
        logger.emit("transport", "connected", "connected")

        self.assertEqual(messages, ["connected", "connected"])
