"""Small change-based logging helpers for the long-running cabinet process."""

from __future__ import annotations

import time
from collections.abc import Callable
from typing import Any

_STARTED_AT = time.monotonic()


def format_runtime_message(message: str) -> str:
    prefix, separator, body = message.partition(" // ")
    category = prefix
    if prefix.startswith("CABINET FRONTEND"):
        category = "FRONTEND"
    elif prefix.startswith(("CABINET VOICE", "VOICE")):
        category = "VOICE"
    elif prefix.startswith("BACKEND"):
        category = "BACKEND"
    elif prefix.startswith("TAP BRIDGE") or prefix in {"SPEAKER", "INTERFERENCE"}:
        category = "AUDIO"
    elif prefix in {"CALLS", "SERVICE"}:
        category = "CALL"
    elif prefix in {"GAME PHASE", "SHIFT", "RUN", "STORY"}:
        category = "GAME"
    elif prefix in {"DIRECTORY", "INPUT", "ROTARY", "CRANK"}:
        category = "INPUT"
    elif prefix in {"PRINTER"}:
        category = "OUTPUT"
    if separator:
        return f"[+{time.monotonic() - _STARTED_AT:.3f}s] [{category}] {body}"
    return f"[+{time.monotonic() - _STARTED_AT:.3f}s] [{category}] {message}"


def log_runtime(message: str) -> None:
    print(format_runtime_message(message), flush=True)


class ChangeLogger:
    """Emit a diagnostic only when the value for a named signal changes."""

    def __init__(self, sink: Callable[[str], None] | None = print) -> None:
        self.sink = log_runtime if sink is print else sink
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
