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
    """Lazy cbor2 adapter so protocol tests need no hardware packages."""

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
    if message["protocol_version"] != 3:
        raise ProtocolValidationError("unsupported input protocol version")
    _require_int_range(message["input_sequence"], "input_sequence", 1, None)
    _require_int_range(message["expected_state_revision"], "expected_state_revision", 0, None)
    input_state = _map(message["input"], "input")
    _require_keys(
        input_state,
        "cord_topology",
        "held_controls",
        "directory_digits",
        "ring_line",
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
    held_keys = {"ptt", "police", "ems", "tap"}
    if set(held) != held_keys:
        raise ProtocolValidationError("held_controls contains unsupported controls")
    for key in held_keys:
        if not isinstance(held.get(key), bool):
            raise ProtocolValidationError(f"held_controls.{key} must be boolean")

    digits = input_state["directory_digits"]
    if (
        not isinstance(digits, list)
        or len(digits) != 4
        or any(type(value) is not int or not 0 <= value <= 9 for value in digits)
    ):
        raise ProtocolValidationError("directory_digits must contain four digits")

    ring_line = input_state["ring_line"]
    if type(ring_line) is not int or not -1 <= ring_line < 12:
        raise ProtocolValidationError("ring_line must be -1 or a subscriber line")

    tuning = _map(input_state["tuning"], "tuning")
    for key in ("coarse", "fine"):
        _require_int_range(tuning.get(key), f"tuning.{key}", 0, 1023)

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
    if message["protocol_version"] != 3:
        raise ProtocolValidationError("unsupported state protocol version")
    _require_int_range(message["input_sequence"], "input_sequence", 1, None)
    _require_int_range(message["state_revision"], "state_revision", 0, None)
    if not isinstance(message["accepted"], bool):
        raise ProtocolValidationError("accepted must be boolean")
    if message["error"] is not None:
        error = _map(message["error"], "error")
        _require_keys(error, "code", "message")
        if not isinstance(error["code"], str) or not isinstance(error["message"], str):
            raise ProtocolValidationError("error.code and error.message must be strings")
    output = _map(message["output"], "output")
    _require_keys(
        output,
        "line_lamps",
        "game_phase",
        "run_generation",
        "clock",
        "speaker_active",
        "interference_level",
        "tap_bridge_audio_active",
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
        or len(lamps) != 12
        or not all(isinstance(value, bool) for value in lamps)
    ):
        raise ProtocolValidationError("output.line_lamps must contain twelve booleans")
    if not isinstance(output["game_phase"], str):
        raise ProtocolValidationError("output.game_phase must be a string")
    _require_int_range(output["run_generation"], "output.run_generation", 0, None)
    if not isinstance(output["speaker_active"], bool):
        raise ProtocolValidationError("output.speaker_active must be boolean")
    if not isinstance(output["tap_bridge_audio_active"], bool):
        raise ProtocolValidationError("output.tap_bridge_audio_active must be boolean")
    _require_int_range(output["interference_level"], "output.interference_level", 0, 100)
    clock = _map(output["clock"], "output.clock")
    _require_keys(clock, "shift", "elapsed_seconds")
    _require_int_range(clock["shift"], "output.clock.shift", 0, None)
    _require_int_range(clock["elapsed_seconds"], "output.clock.elapsed_seconds", 0, None)
    tuning = _map(output["tuning"], "output.tuning")
    _require_keys(tuning, "coarse", "fine")
    for key in ("coarse", "fine"):
        _require_int_range(tuning[key], f"output.tuning.{key}", 0, 1023)
    if not isinstance(output["directory_pages"], list) or not all(
        isinstance(page, dict) for page in output["directory_pages"]
    ):
        raise ProtocolValidationError("output.directory_pages must be a list")
    calls = output["calls"]
    if not isinstance(calls, list):
        raise ProtocolValidationError("output.calls must be a list")
    for call in calls:
        call_map = _map(call, "output.calls entry")
        _require_keys(call_map, "caller_line", "requested_callee_line", "phase")
        _require_int_range(call_map["caller_line"], "calls.caller_line", 0, 11)
        _require_int_range(call_map["requested_callee_line"], "calls.requested_callee_line", 0, 11)
        if not isinstance(call_map["phase"], str):
            raise ProtocolValidationError("calls.phase must be a string")
    service_call = output["service_call"]
    if service_call is not None:
        service_map = _map(service_call, "output.service_call")
        _require_keys(service_map, "service", "phase")
        if not all(isinstance(service_map[key], str) for key in ("service", "phase")):
            raise ProtocolValidationError("service_call.service and phase must be strings")
    monitoring = output["tap_bridge_monitoring"]
    if monitoring is not None:
        monitoring_map = _map(monitoring, "output.tap_bridge_monitoring")
        _require_keys(monitoring_map, "caller_line", "callee_line", "caller_tap_port", "callee_tap_port")
        for key in ("caller_line", "callee_line"):
            _require_int_range(monitoring_map[key], f"tap_bridge_monitoring.{key}", 0, 11)
        for key in ("caller_tap_port", "callee_tap_port"):
            _require_int_range(monitoring_map[key], f"tap_bridge_monitoring.{key}", 1, 2)
    printer_output = output["printer_output"]
    if not isinstance(printer_output, list):
        raise ProtocolValidationError("output.printer_output must be a list")
    for entry in printer_output:
        entry_map = _map(entry, "output.printer_output entry")
        _require_keys(entry_map, "entry_id", "text")
        _require_int_range(entry_map["entry_id"], "printer_output.entry_id", None, None)
        if not isinstance(entry_map["text"], str):
            raise ProtocolValidationError("printer_output.text must be a string")
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
    for entry in debug["messages"]:
        entry_map = _map(entry, "output.debug.messages entry")
        _require_keys(entry_map, "code", "message")
        if not all(isinstance(entry_map[key], str) for key in ("code", "message")):
            raise ProtocolValidationError("debug message code and message must be strings")


def _require_keys(value: dict[str, Any], *keys: str) -> None:
    missing = [key for key in keys if key not in value]
    if missing:
        raise ProtocolValidationError(f"missing wire fields: {', '.join(missing)}")


def _map(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolValidationError(f"{name} must be a map")
    return value


def _require_int_range(value: Any, name: str, minimum: int | None, maximum: int | None) -> None:
    if (
        type(value) is not int
        or (minimum is not None and value < minimum)
        or (maximum is not None and value > maximum)
    ):
        bounds = ""
        if minimum is not None:
            bounds += f" >= {minimum}"
        if maximum is not None:
            bounds += f" <= {maximum}"
        raise ProtocolValidationError(f"{name} must be an integer{bounds}")


def _valid_port(value: Any) -> bool:
    if value in {"operator", "ring_generator"}:
        return True
    if not isinstance(value, str):
        return False
    if value.startswith("subscriber_"):
        suffix = value.removeprefix("subscriber_")
        return (
            suffix.isdigit()
            and 0 <= int(suffix) < 12
            and (suffix == "0" or not suffix.startswith("0"))
        )
    if value.startswith("tap_"):
        suffix = value.removeprefix("tap_")
        return suffix in {"1", "2"}
    return False


class BackendClient:
    def __init__(self, connection: Any, codec: Codec) -> None:
        self.connection = connection
        self.codec = codec

    @classmethod
    def connect(cls, address: tuple[str, int], timeout: float = 5.0) -> BackendClient:
        connection = socket.create_connection(address, timeout=timeout)
        try:
            connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
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
