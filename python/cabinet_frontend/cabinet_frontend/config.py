"""Configuration for the Raspberry Pi Cabinet Frontend."""

from __future__ import annotations

import os
from dataclasses import dataclass, field

DEFAULT_PATCH_PANEL_PIN_TO_PORT = {
    **{pin: f"subscriber_{pin}" for pin in range(12)},
    12: "operator",
    13: "ring_generator",
    14: "tap_1",
    15: "tap_2",
}


def parse_backend_address(value: str) -> tuple[str, int]:
    host, separator, port_text = value.rpartition(":")
    if not separator or not host or not port_text:
        raise ValueError("backend address must be HOST:PORT")
    try:
        port = int(port_text)
    except ValueError as error:
        raise ValueError("backend address port must be numeric") from error
    if not 1 <= port <= 65535:
        raise ValueError("backend address port must be between 1 and 65535")
    return host, port


@dataclass(frozen=True)
class HardwareConfig:
    backend_address: tuple[str, int] = ("127.0.0.1", 7878)
    voice_upload_address: tuple[str, int] = ("127.0.0.1", 7883)
    voice_playback_gain: float = 1.5
    # Raspberry Pi BCM GPIO numbers. Keep every cabinet connection here so
    # hardware changes do not require edits across component implementations.
    mcp_sda: int = 2
    mcp_scl: int = 3
    i2c_address: int = 0x20
    mcp_patch_panel_pins: tuple[int, ...] = tuple(range(16))
    spi_bus: int = 0
    spi_device: int = 0
    spi_speed_hz: int = 10_000_000
    ws2812_din: int = 12
    ws2812_device: str = "/dev/leds0"
    ws2812_count: int = 16
    tm1637_clk: int = 27
    tm1637_dio: int = 17
    epaper_button_pins: tuple[int, ...] = (26, 1, 16, 20)
    max98357a_lrc: int = 19
    max98357a_bclk: int = 18
    max98357a_din: int = 21
    epaper_busy: int = 25
    epaper_rst: int = 24
    epaper_dc: int = 23
    epaper_cs: int = 8
    epaper_sclk: int = 11
    epaper_sda: int = 10
    rotary_s1: int = 14
    rotary_s2: int = 15
    toggle_switch_pins: tuple[int, ...] = (5, 22, 9, 0)
    printer_device: str = "/dev/usb/lp0"
    control_debounce_ms: int = 50
    control_poll_interval: float = 0.01
    directory_digits: tuple[int, int, int, int] = (0, 0, 0, 1)
    pin_to_port: dict[int, str] = field(
        default_factory=lambda: dict(DEFAULT_PATCH_PANEL_PIN_TO_PORT)
    )
    line_led_map: dict[int, int] = field(default_factory=lambda: dict(enumerate(range(16))))
    line_lamp_count: int = 12
    poll_interval: float = 0.1
    input_status_interval: float = 5.0
    pair_scan_interval: float = 0.5
    pair_line_interval: float = 0.02
    epaper_page_interval: float = 8.0
    epaper_update_delay: float = 0.75
    crank_detents_per_rotation: int = 2
    epaper_rotation: int = 90
    tuning_coarse: int = 0
    tuning_fine: int = 0

    def __post_init__(self) -> None:
        if not self.voice_playback_gain > 0:
            raise ValueError("voice_playback_gain must be positive")
        if self.ws2812_count <= 0 or self.line_lamp_count <= 0:
            raise ValueError("LED counts must be positive")
        if self.crank_detents_per_rotation <= 0:
            raise ValueError("crank_detents_per_rotation must be positive")
        if self.control_debounce_ms < 0:
            raise ValueError("control_debounce_ms must be non-negative")
        if (
            self.input_status_interval <= 0
            or self.pair_scan_interval <= 0
            or self.pair_line_interval <= 0
            or self.control_poll_interval <= 0
        ):
            raise ValueError("input logging and pair scan intervals must be positive")
        if self.epaper_page_interval <= 0:
            raise ValueError("epaper_page_interval must be positive")
        if self.epaper_update_delay < 0:
            raise ValueError("epaper_update_delay must be non-negative")
        if len(self.mcp_patch_panel_pins) != 16 or len(set(self.mcp_patch_panel_pins)) != 16:
            raise ValueError("mcp_patch_panel_pins must contain sixteen unique pins")
        if len(self.toggle_switch_pins) != 4 or len(set(self.toggle_switch_pins)) != 4:
            raise ValueError("toggle_switch_pins must contain four unique pins")
        if len(self.epaper_button_pins) != 4 or len(set(self.epaper_button_pins)) != 4:
            raise ValueError("epaper_button_pins must contain four unique pins")
        for value in (self.tuning_coarse, self.tuning_fine):
            if type(value) is not int or not 0 <= value <= 1023:
                raise ValueError("tuning values must be integers between 0 and 1023")

    @classmethod
    def from_environment(cls) -> HardwareConfig:
        backend_host = os.environ.get("NN_BACKEND_HOST")
        address = os.environ.get(
            "NN_BACKEND_ADDRESS",
            f"{backend_host}:7878" if backend_host else "127.0.0.1:7878",
        )
        host, port = parse_backend_address(address)
        return cls(
            backend_address=(host, port),
            voice_upload_address=(host, 7883),
            pair_scan_interval=float(os.environ.get("NN_PAIR_SCAN_INTERVAL", "0.5")),
            epaper_rotation=int(os.environ.get("NN_EPAPER_ROTATION", "90")),
            epaper_update_delay=float(os.environ.get("NN_EPAPER_UPDATE_DELAY", "0.75")),
        )
