from __future__ import annotations

from collections.abc import Sequence


class Ws2812LineLamps:
    """Adapter for the Pi's custom /dev/leds0 driver."""

    def __init__(
        self,
        count: int = 8,
        line_led_map: dict[int, int] | None = None,
        brightness: int = 28,
    ) -> None:
        import ws2812

        self.driver = ws2812
        self.count = count
        self.line_led_map = line_led_map or {index: index for index in range(count)}
        self.driver.set_brightness(brightness)

    def set_lines(self, lines: Sequence[bool]) -> None:
        value = 0
        for line, active in enumerate(lines):
            led = self.line_led_map.get(line)
            if active and led is not None and 0 <= led < self.count:
                value |= 1 << led
        self.driver.set(value)

    def close(self) -> None:
        self.driver.set(0)
