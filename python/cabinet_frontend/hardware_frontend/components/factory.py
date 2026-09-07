"""Construct either safe dummy components or Pi-backed components."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from typing import TextIO

from ..config import HardwareConfig
from .audio_dummy import StdoutAudio
from .epaper_dummy import StdoutEpaper
from .inputs_dummy import StdoutRotary, StdoutTopologyScanner
from .keyboard import KeyboardControls, NoopControls
from .line_lamps_dummy import StdoutLineLamps
from .printer_dummy import StdoutPrinter
from .seven_segment_dummy import StdoutSevenSegment


@dataclass
class ComponentBundle:
    line_lamps: object
    seven_segment: object
    epaper: object
    rotary: object
    scanner: object
    controls: object
    printer: object
    audio: object
    extra_closers: list[object]

    def close(self) -> None:
        for component in (
            self.line_lamps,
            self.seven_segment,
            self.epaper,
            self.rotary,
            self.scanner,
            self.controls,
            self.printer,
            self.audio,
            *self.extra_closers,
        ):
            close = getattr(component, "close", None)
            if close is not None:
                close()
            else:
                deinit = getattr(component, "deinit", None)
                if deinit is not None:
                    deinit()


def build_dummy_components(output: TextIO | None = None) -> ComponentBundle:
    output = output or sys.stdout
    return ComponentBundle(
        line_lamps=StdoutLineLamps(output),
        seven_segment=StdoutSevenSegment(output),
        epaper=StdoutEpaper(output),
        rotary=StdoutRotary(output),
        scanner=StdoutTopologyScanner(output),
        controls=NoopControls(),
        printer=StdoutPrinter(output),
        audio=StdoutAudio(output),
        extra_closers=[],
    )


def build_real_components(config: HardwareConfig) -> ComponentBundle:
    """Build Pi components. Imports intentionally happen only in real mode."""
    import board
    import busio
    from adafruit_mcp230xx.mcp23017 import MCP23017

    from .epaper import EpaperDirectoryDisplay
    from .pair_detector import McpPairDetector
    from .rotary_encoder import McpRotaryEncoder
    from .seven_segment import Tm1637Display
    from .ws2812 import Ws2812LineLamps

    i2c = busio.I2C(board.SCL, board.SDA)
    mcp = MCP23017(i2c, address=config.i2c_address)
    return ComponentBundle(
        line_lamps=Ws2812LineLamps(config.ws2812_count, config.line_led_map),
        seven_segment=Tm1637Display(config.tm1637_clk, config.tm1637_dio),
        epaper=EpaperDirectoryDisplay(mcp),
        rotary=McpRotaryEncoder(mcp, config.encoder_s1, config.encoder_s2),
        scanner=McpPairDetector(mcp),
        controls=KeyboardControls() if config.keyboard_controls else NoopControls(),
        printer=StdoutPrinter(sys.stdout),
        audio=StdoutAudio(sys.stdout),
        extra_closers=[i2c],
    )
