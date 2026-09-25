from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest.mock import patch

from cabinet_frontend.components.epaper import wrap_text


class EpaperLayoutTests(unittest.TestCase):
    def test_wraps_only_at_word_boundaries(self) -> None:
        lines = wrap_text("A long directory entry", 10, len)
        long_word_lines = wrap_text("ABCDEFGHIJK", 5, len)

        self.assertEqual(lines, ["A long", "directory", "entry"])
        self.assertEqual(long_word_lines, ["ABCDEFGHIJK"])

    def test_epaper_uses_numeric_spi_configuration(self) -> None:
        from cabinet_frontend.components.epaper import EpaperDirectoryDisplay

        device = SimpleNamespace(init=lambda: None)
        epaper_module = SimpleNamespace(EPaper=lambda **kwargs: device)
        pil_image = SimpleNamespace()
        pil_draw = SimpleNamespace(Draw=lambda image: object())
        pil_font = SimpleNamespace()
        pil = SimpleNamespace(Image=pil_image, ImageDraw=pil_draw, ImageFont=pil_font)
        with patch.dict(
            "sys.modules",
            {
                "epaper": epaper_module,
                "PIL": pil,
                "PIL.Image": pil_image,
                "PIL.ImageDraw": pil_draw,
                "PIL.ImageFont": pil_font,
            },
        ), patch.object(epaper_module, "EPaper", wraps=epaper_module.EPaper) as epaper:
            EpaperDirectoryDisplay(spi_bus=0, spi_device=0, spi_speed_hz=10_000_000)

        epaper.assert_called_once_with(bus=0, device=0, speed=10_000_000)
