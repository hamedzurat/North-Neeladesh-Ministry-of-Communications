"""Explicit Raspberry Pi hardware connection test; never connects to the backend."""

from __future__ import annotations

import argparse
import sys
import time
from collections.abc import Callable
from typing import TextIO

from .components.factory import ComponentBundle, build_real_components
from .config import HardwareConfig


def exercise_components(
    components: ComponentBundle,
    output: TextIO,
    sleep: Callable[[float], None] = time.sleep,
    led_delay: float = 0.2,
) -> int:
    """Exercise every available output once and report one input sample."""
    print("HARDWARE // WS2812 walking test", file=output)
    for index in range(8):
        lines = [False] * 8
        lines[index] = True
        components.line_lamps.set_lines(lines)
        print(f"HARDWARE // LED {index} on", file=output)
        sleep(led_delay)
    components.line_lamps.set_lines([False] * 8)

    print("HARDWARE // TM1637 8888", file=output)
    components.seven_segment.show("8888")
    sleep(led_delay)
    components.seven_segment.show("0123")

    print("HARDWARE // e-paper directory render", file=output)
    components.epaper.show_directory(
        [
            {
                "page_number": 1,
                "heading": "HARDWARE TEST",
                "lines": [
                    "Directory text wrapping test",
                    "MCP23017 / SPI / e-paper OK",
                ],
            }
        ]
    )

    pairs = components.scanner.find_pairs()
    print(f"HARDWARE // pair detector found {pairs!r}", file=output)
    direction = components.rotary.read()
    print(f"HARDWARE // rotary sample {direction}", file=output)
    return direction


def run_hardware_smoke(
    config: HardwareConfig,
    output: TextIO = sys.stdout,
    duration: float = 10.0,
    led_delay: float = 0.2,
    sleep: Callable[[float], None] = time.sleep,
    clock: Callable[[], float] = time.monotonic,
) -> None:
    components = build_real_components(config)
    try:
        exercise_components(components, output, sleep, led_delay)
        print("HARDWARE // turn the rotary encoder during the listening window", file=output)
        deadline = clock() + duration
        while clock() < deadline:
            direction = components.rotary.read()
            if direction:
                print(f"HARDWARE // rotary direction {direction}", file=output)
            sleep(0.01)
    finally:
        components.close()
        print("HARDWARE // all outputs cleared and devices closed", file=output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--real", action="store_true", help="required safety confirmation")
    parser.add_argument("--duration", type=float, default=10.0)
    parser.add_argument("--led-delay", type=float, default=0.2)
    args = parser.parse_args()
    if not args.real:
        parser.error("hardware smoke tests require explicit --real")
    config = HardwareConfig.from_environment()
    config = HardwareConfig(**{**config.__dict__, "mode": "real"})
    run_hardware_smoke(config, duration=args.duration, led_delay=args.led_delay)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
