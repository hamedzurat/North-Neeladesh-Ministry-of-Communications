"""Small change-based logging helpers for the long-running cabinet process."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any


class ChangeLogger:
    """Emit a diagnostic only when the value for a named signal changes."""

    def __init__(self, sink: Callable[[str], None] | None = print) -> None:
        self.sink = sink
        self._values: dict[str, Any] = {}

    def emit(self, key: str, value: Any, message: str) -> None:
        if self.sink is None or key in self._values and self._values[key] == value:
            return
        self._values[key] = value
        try:
            self.sink(message)
        except Exception:  # noqa: BLE001 - diagnostics must never stop the runtime
            return

    def reset(self, key: str) -> None:
        self._values.pop(key, None)
