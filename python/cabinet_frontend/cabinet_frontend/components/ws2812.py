from __future__ import annotations

import os
from collections.abc import Sequence


class Ws2812LineLamps:
    """Full RGB adapter for Raspberry Pi's /dev/leds0 WS2812 driver."""

    def __init__(
        self,
        count: int = 8,
        line_led_map: dict[int, int] | None = None,
        brightness: int = 28,
        device: str = "/dev/leds0",
    ) -> None:
        self.count = count
        self.line_led_map = line_led_map or {index: index for index in range(count)}
        if not 0 <= brightness <= 255:
            raise ValueError("brightness must be between 0 and 255")
        self._device = os.open(device, os.O_WRONLY)
        self.set_brightness(brightness)

    def set_brightness(self, brightness: int) -> None:
        if not 0 <= brightness <= 255:
            raise ValueError("brightness must be between 0 and 255")
        os.pwrite(self._device, bytes((brightness,)), 0)

    def set_pixels(
        self,
        pixels: Sequence[tuple[int, int, int]],
        brightness: int | None = None,
    ) -> None:
        if len(pixels) != self.count:
            raise ValueError(f"expected {self.count} pixels, got {len(pixels)}")
        if brightness is not None:
            self.set_brightness(brightness)
        physical_pixels = [(0, 0, 0)] * self.count
        for logical, pixel in enumerate(pixels):
            physical = self.line_led_map.get(logical, logical)
            if 0 <= physical < self.count:
                physical_pixels[physical] = pixel
        payload = bytearray()
        for red, green, blue in physical_pixels:
            for channel in (red, green, blue):
                if not 0 <= channel <= 255:
                    raise ValueError("pixel channels must be between 0 and 255")
            payload.extend((red, green, blue, 0))
        os.pwrite(self._device, payload, 0)

    def set_lines(self, lines: Sequence[bool]) -> None:
        pixels = [(0, 0, 0)] * self.count
        for line, active in enumerate(lines):
            led = self.line_led_map.get(line)
            if active and led is not None and 0 <= led < self.count:
                pixels[led] = (255, 255, 255)
        self.set_pixels(pixels)

    def close(self) -> None:
        try:
            os.pwrite(self._device, bytes(self.count * 4), 0)
        finally:
            os.close(self._device)
