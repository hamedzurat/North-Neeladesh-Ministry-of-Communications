"""Configuration for the Raspberry Pi Cabinet Frontend."""

from __future__ import annotations

import os
from dataclasses import dataclass, field

DEFAULT_PIN_TO_PORT = {pin: f"subscriber_{pin}" for pin in range(8)}


@dataclass(frozen=True)
class HardwareConfig:
    backend_address: tuple[str, int] = ("127.0.0.1", 7878)
    mode: str = "dummy"
    i2c_address: int = 0x20
    spi_bus: int = 0
    spi_device: int = 0
    spi_speed_hz: int = 10_000_000
    encoder_s1: int = 11
    encoder_s2: int = 12
    ws2812_count: int = 8
    tm1637_clk: int = 27
    tm1637_dio: int = 17
    directory_digits: tuple[int, int, int, int] = (0, 0, 0, 1)
    pin_to_port: dict[int, str] = field(default_factory=lambda: dict(DEFAULT_PIN_TO_PORT))
    line_led_map: dict[int, int] = field(default_factory=lambda: dict(enumerate(range(8))))
    poll_interval: float = 0.1
    pair_scan_interval: float = 2.0
    epaper_page_interval: float = 8.0
    keyboard_controls: bool = False
    crank_detents_per_rotation: int = 16

    @classmethod
    def from_environment(cls) -> HardwareConfig:
        address = os.environ.get("NN_BACKEND_ADDRESS", "127.0.0.1:7878")
        host, separator, port_text = address.rpartition(":")
        if not separator or not host:
            raise ValueError("NN_BACKEND_ADDRESS must be HOST:PORT")
        try:
            port = int(port_text)
        except ValueError as error:
            raise ValueError("NN_BACKEND_ADDRESS must use a numeric port") from error
        mode = os.environ.get("NN_HARDWARE_MODE", "dummy").lower()
        if mode not in {"dummy", "real"}:
            raise ValueError("NN_HARDWARE_MODE must be dummy or real")
        keyboard_controls = os.environ.get("NN_KEYBOARD_CONTROLS", "0") == "1"
        return cls(backend_address=(host, port), mode=mode, keyboard_controls=keyboard_controls)
