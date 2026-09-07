"""Framed CBOR transport used by the Odin and Cabinet Frontends."""

from __future__ import annotations

import socket
import struct
from typing import Any, Protocol

MAX_FRAME_SIZE = 4 * 1_048_576


class Codec(Protocol):
    def encode(self, value: Any) -> bytes: ...

    def decode(self, payload: bytes) -> Any: ...


class CborCodec:
    """Lazy cbor2 adapter so dummy mode and tests need no hardware packages."""

    def __init__(self) -> None:
        try:
            import cbor2
        except ImportError as error:
            raise RuntimeError(
                "cbor2 is required for the Pi runtime; install the uv project dependencies"
            ) from error
        self._cbor2 = cbor2

    def encode(self, value: Any) -> bytes:
        return self._cbor2.dumps(value)

    def decode(self, payload: bytes) -> Any:
        return self._cbor2.loads(payload)


def frame(payload: bytes) -> bytes:
    if len(payload) > MAX_FRAME_SIZE:
        raise ValueError(f"payload exceeds {MAX_FRAME_SIZE} bytes")
    return struct.pack(">I", len(payload)) + payload


def send_frame(connection: Any, payload: bytes) -> None:
    connection.sendall(frame(payload))


def receive_exact(connection: Any, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining:
        chunk = connection.recv(remaining)
        if not chunk:
            raise ConnectionError("backend closed the connection")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def receive_frame(connection: Any) -> bytes:
    length = struct.unpack(">I", receive_exact(connection, 4))[0]
    if length > MAX_FRAME_SIZE:
        raise ValueError(f"payload exceeds {MAX_FRAME_SIZE} bytes")
    return receive_exact(connection, length)


class ProtocolValidationError(ValueError):
    """A message does not satisfy the documented frontend wire contract."""


def validate_input_message(message: dict[str, Any]) -> None:
    _require_keys(message, "protocol_version", "input_sequence", "expected_state_revision", "input")
    if message["protocol_version"] != 1:
        raise ProtocolValidationError("unsupported input protocol version")
    if not isinstance(message["input_sequence"], int) or message["input_sequence"] <= 0:
        raise ProtocolValidationError("input_sequence must be a positive integer")
    input_state = _map(message["input"], "input")
    _require_keys(
        input_state,
        "cord_topology",
        "held_controls",
        "directory_digits",
        "crank_rotation_timestamps",
        "tuning",
        "debug",
    )
    topology = input_state["cord_topology"]
    if not isinstance(topology, list) or len(topology) > 8:
        raise ProtocolValidationError("cord_topology must contain at most eight cords")
    endpoints: set[str] = set()
    for cord in topology:
        cord_map = _map(cord, "cord")
        _require_keys(cord_map, "first", "second")
        for endpoint in (cord_map["first"], cord_map["second"]):
            if not _valid_port(endpoint) or endpoint in endpoints:
                raise ProtocolValidationError(
                    "cord topology contains an invalid or repeated endpoint"
                )
            endpoints.add(endpoint)

    held = _map(input_state["held_controls"], "held_controls")
    for key in ("ptt", "police", "ems", "fire", "tap_1", "tap_2"):
        if not isinstance(held.get(key), bool):
            raise ProtocolValidationError(f"held_controls.{key} must be boolean")

    digits = input_state["directory_digits"]
    if (
        not isinstance(digits, list)
        or len(digits) != 4
        or any(not isinstance(value, int) or not 0 <= value <= 9 for value in digits)
    ):
        raise ProtocolValidationError("directory_digits must contain four digits")

    timestamps = input_state["crank_rotation_timestamps"]
    if (
        not isinstance(timestamps, list)
        or len(timestamps) > 4
        or any(not isinstance(value, int) or value < 0 for value in timestamps)
    ):
        raise ProtocolValidationError(
            "crank_rotation_timestamps must contain at most four timestamps"
        )
    if timestamps != sorted(timestamps):
        raise ProtocolValidationError("crank_rotation_timestamps must be chronological")

    tuning = _map(input_state["tuning"], "tuning")
    for key in ("coarse", "fine"):
        if not isinstance(tuning.get(key), int) or not 0 <= tuning[key] <= 1023:
            raise ProtocolValidationError(f"tuning.{key} must be between 0 and 1023")

    debug = _map(input_state["debug"], "debug")
    if not isinstance(debug.get("firmware_version"), (str, type(None))):
        raise ProtocolValidationError("debug.firmware_version must be string or null")
    if not isinstance(debug.get("transport_connected"), bool):
        raise ProtocolValidationError("debug.transport_connected must be boolean")
    if not isinstance(debug.get("device_faults"), list) or not all(
        isinstance(value, str) for value in debug["device_faults"]
    ):
        raise ProtocolValidationError("debug.device_faults must be strings")


def validate_state_message(message: dict[str, Any]) -> None:
    _require_keys(
        message,
        "protocol_version",
        "input_sequence",
        "accepted",
        "error",
        "state_revision",
        "output",
    )
    if message["protocol_version"] != 1:
        raise ProtocolValidationError("unsupported state protocol version")
    if not isinstance(message["accepted"], bool):
        raise ProtocolValidationError("accepted must be boolean")
    if message["error"] is not None:
        error = _map(message["error"], "error")
        _require_keys(error, "code", "message")
    output = _map(message["output"], "output")
    _require_keys(
        output,
        "line_lamps",
        "game_phase",
        "clock",
        "speaker_active",
        "tuning",
        "directory_pages",
        "printer_output",
        "call",
        "calls",
        "service_call",
        "tap_bridge_monitoring",
        "shift",
        "debug",
    )
    lamps = output["line_lamps"]
    if (
        not isinstance(lamps, list)
        or len(lamps) != 16
        or not all(isinstance(value, bool) for value in lamps)
    ):
        raise ProtocolValidationError("output.line_lamps must contain sixteen booleans")
    _map(output["clock"], "output.clock")
    _require_keys(output["clock"], "shift", "elapsed_seconds")
    _map(output["tuning"], "output.tuning")
    _require_keys(output["tuning"], "coarse", "fine")
    if not isinstance(output["directory_pages"], list):
        raise ProtocolValidationError("output.directory_pages must be a list")
    if not isinstance(output["printer_output"], list) or not isinstance(output["calls"], list):
        raise ProtocolValidationError("output printer_output and calls must be lists")
    shift = _map(output["shift"], "output.shift")
    _require_keys(
        shift,
        "number",
        "phase",
        "active_call_count",
        "completed_routings",
        "required_service_calls",
        "completed_service_calls",
        "service_errors",
        "service_error_counts",
    )
    debug = _map(output["debug"], "output.debug")
    _require_keys(debug, "messages")
    if not isinstance(debug["messages"], list):
        raise ProtocolValidationError("output.debug.messages must be a list")


def _require_keys(value: dict[str, Any], *keys: str) -> None:
    missing = [key for key in keys if key not in value]
    if missing:
        raise ProtocolValidationError(f"missing wire fields: {', '.join(missing)}")


def _map(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolValidationError(f"{name} must be a map")
    return value


def _valid_port(value: Any) -> bool:
    if value in {"operator", "ring_generator"}:
        return True
    if not isinstance(value, str):
        return False
    if value.startswith("subscriber_"):
        suffix = value.removeprefix("subscriber_")
        return (
            suffix.isdigit()
            and 0 <= int(suffix) < 16
            and (suffix == "0" or not suffix.startswith("0"))
        )
    if value.startswith("tap_"):
        suffix = value.removeprefix("tap_")
        return suffix in {"1", "2", "3", "4"}
    return False


class BackendClient:
    def __init__(self, connection: Any, codec: Codec) -> None:
        self.connection = connection
        self.codec = codec

    @classmethod
    def connect(cls, address: tuple[str, int], timeout: float = 5.0) -> BackendClient:
        connection = socket.create_connection(address, timeout=timeout)
        try:
            codec = CborCodec()
        except Exception:
            connection.close()
            raise
        return cls(connection, codec)

    def exchange(self, message: dict[str, Any]) -> dict[str, Any]:
        validate_input_message(message)
        send_frame(self.connection, self.codec.encode(message))
        response = self.codec.decode(receive_frame(self.connection))
        if not isinstance(response, dict):
            raise ProtocolValidationError("backend response must be a map")
        validate_state_message(response)
        return response

    def close(self) -> None:
        self.connection.close()
