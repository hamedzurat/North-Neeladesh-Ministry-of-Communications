"""Wire-independent Cabinet Frontend state models."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class HeldControls:
    ptt: bool = False
    police: bool = False
    ems: bool = False
    tap: bool = False

    def to_wire(self) -> dict[str, bool]:
        return {
            "ptt": self.ptt,
            "police": self.police,
            "ems": self.ems,
            "tap": self.tap,
        }


@dataclass
class PhysicalInput:
    cord_topology: list[dict[str, str]] = field(default_factory=list)
    held_controls: HeldControls = field(default_factory=HeldControls)
    directory_digits: list[int] = field(default_factory=lambda: [0, 0, 0, 1])
    crank_rotation_timestamps: list[int] = field(default_factory=list)
    tuning: dict[str, int] = field(default_factory=lambda: {"coarse": 0, "fine": 0})


def input_message(
    physical: PhysicalInput,
    sequence: int,
    expected_state_revision: int,
    firmware_version: str,
    device_faults: list[str],
) -> dict[str, Any]:
    timestamps = physical.crank_rotation_timestamps[-4:]
    timestamps = [0] * (4 - len(timestamps)) + timestamps
    return {
        "protocol_version": 2,
        "input_sequence": sequence,
        "expected_state_revision": expected_state_revision,
        "input": {
            "cord_topology": physical.cord_topology,
            "held_controls": physical.held_controls.to_wire(),
            "directory_digits": physical.directory_digits,
            "crank_rotation_timestamps": timestamps,
            "tuning": physical.tuning,
            "debug": {
                "firmware_version": firmware_version,
                "transport_connected": True,
                "device_faults": device_faults,
            },
        },
    }
