"""Construct the Raspberry Pi cabinet hardware components."""

from __future__ import annotations

import sys
from dataclasses import dataclass

from ..config import HardwareConfig


@dataclass
class ComponentBundle:
    line_lamps: object
    seven_segment: object
    epaper: object
    rotary: object
    scanner: object
    controls: object
    printer: object
    extra_closers: list[object]

    def close(self) -> None:
        errors: list[Exception] = []
        for component in (
            self.line_lamps,
            self.seven_segment,
            self.epaper,
            self.rotary,
            self.scanner,
            self.controls,
            self.printer,
            *self.extra_closers,
        ):
            close = getattr(component, "close", None)
            try:
                if close is not None:
                    close()
                else:
                    deinit = getattr(component, "deinit", None)
                    if deinit is not None:
                        deinit()
            except Exception as error:  # noqa: BLE001 - clean every hardware resource
                errors.append(error)
        if errors:
            raise RuntimeError(f"{len(errors)} hardware components failed to close") from errors[0]


def build_real_components(config: HardwareConfig) -> ComponentBundle:
    """Build all Pi components with imports deferred until hardware startup."""
    import board
    import busio
    from adafruit_mcp230xx.mcp23017 import MCP23017

    from .epaper import EpaperDirectoryDisplay
    from .gpio_controls import GpioControls
    from .pair_detector import McpPairDetector
    from .printer import DevicePrinter
    from .rotary_encoder import GpioRotaryEncoder
    from .seven_segment import Tm1637Display
    from .ws2812 import Ws2812LineLamps

    i2c = busio.I2C(
        getattr(board, f"D{config.mcp_scl}"),
        getattr(board, f"D{config.mcp_sda}"),
    )
    initialized: list[object] = [i2c]
    try:
        mcp = MCP23017(i2c, address=config.i2c_address)
        line_lamps = Ws2812LineLamps(config.ws2812_count, config.line_led_map)
        initialized.append(line_lamps)
        seven_segment = Tm1637Display(config.tm1637_clk, config.tm1637_dio)
        initialized.append(seven_segment)
        epaper = EpaperDirectoryDisplay(
            spi_bus=config.spi_bus,
            spi_device=config.spi_device,
            spi_speed_hz=config.spi_speed_hz,
            rotation=config.epaper_rotation,
        )
        initialized.append(epaper)
        rotary = GpioRotaryEncoder(config.rotary_s1, config.rotary_s2)
        initialized.append(rotary)
        scanner = McpPairDetector(mcp, config.mcp_patch_panel_pins)
        initialized.append(scanner)
        controls = GpioControls(
            config.toggle_switch_pins,
            config.epaper_button_pins,
            config.directory_digits,
            debounce_ms=0,
        )
        printer = DevicePrinter(config.printer_device)
        initialized.append(controls)
        return ComponentBundle(
            line_lamps=line_lamps,
            seven_segment=seven_segment,
            epaper=epaper,
            rotary=rotary,
            scanner=scanner,
            controls=controls,
            printer=printer,
            extra_closers=[i2c],
        )
    except Exception:
        for component in reversed(initialized):
            try:
                close = getattr(component, "close", None)
                if close is not None:
                    close()
                else:
                    deinit = getattr(component, "deinit", None)
                    if deinit is not None:
                        deinit()
            except Exception as error:  # noqa: BLE001 - preserve the construction failure
                print(
                    f"CABINET FRONTEND // cleanup failed error={type(error).__name__}: {error}",
                    file=sys.stderr,
                    flush=True,
                )
        raise
