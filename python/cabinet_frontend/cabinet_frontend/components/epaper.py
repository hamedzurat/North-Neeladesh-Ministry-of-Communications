from __future__ import annotations

from collections.abc import Callable, Sequence
from pathlib import Path


def wrap_text(text: str, max_width: int, measure: Callable[[str], int]) -> list[str]:
    """Wrap words to a pixel width, hard-wrapping words wider than the screen."""
    if not text:
        return [""]
    result: list[str] = []
    for paragraph in text.splitlines() or [""]:
        words = paragraph.split()
        if not words:
            result.append("")
            continue
        current = ""
        while words:
            candidate = f"{current} {words[0]}".strip()
            if current and measure(candidate) > max_width:
                result.append(current)
                current = ""
                continue
            if not current and measure(candidate) > max_width:
                word = words.pop(0)
                chunk = ""
                for character in word:
                    if chunk and measure(chunk + character) > max_width:
                        result.append(chunk)
                        chunk = ""
                    chunk += character
                current = chunk
                continue
            current = candidate
            words.pop(0)
        if current:
            result.append(current)
    return result


class EpaperDirectoryDisplay:
    WIDTH = 200
    HEIGHT = 200
    MARGIN = 6
    FONT_SIZE = 10
    LINE_HEIGHT = 13

    def __init__(
        self,
        spi_bus: int = 0,
        spi_device: int = 0,
        spi_speed_hz: int = 10_000_000,
        rotation: int = 0,
        font_path: str | None = None,
        avatar_dir: str | None = None,
    ) -> None:
        import epaper
        from PIL import Image, ImageDraw, ImageFont

        self._image_module = Image
        self._draw_module = ImageDraw
        self._font_module = ImageFont
        self.device = epaper.EPaper(bus=spi_bus, device=spi_device, speed=spi_speed_hz)
        self.device.init()
        self.rotation = rotation % 360
        self.font_path = font_path or "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
        self.avatar_dir = Path(avatar_dir) if avatar_dir else self._default_avatar_dir()
        self._avatars: dict[int, object | None] = {}

    def show_directory(self, pages: Sequence[dict[str, object]], page_index: int = 0) -> None:
        image = self._image_module.new("1", (self.WIDTH, self.HEIGHT), 255)
        draw = self._draw_module.Draw(image)
        font = self._load_font()
        page_count = len(pages)
        if not page_count:
            draw.text((self.MARGIN, self.MARGIN), "NO DIRECTORY DATA", font=font, fill=0)
            self._show_image(image)
            return

        selected_index = max(0, min(page_index, page_count - 1))
        page = pages[selected_index]
        directory_id = page.get("directory_id")
        if isinstance(directory_id, int):
            draw.text(
                (self.MARGIN, self.MARGIN),
                f"ID // {directory_id:04}",
                font=font,
                fill=0,
            )
            avatar = self._load_avatar(directory_id)
            if avatar is not None:
                image.paste(avatar, (self.WIDTH - self.MARGIN - 48, 0), avatar)
        y = 54 if isinstance(directory_id, int) else self.MARGIN + self.LINE_HEIGHT + 3
        measure = lambda value: self._text_width(draw, value, font)
        lines = page.get("lines", [])
        if lines and str(lines[0]).upper().startswith("SUBSCRIBER ID "):
            lines = lines[1:]
        for raw_line in lines:
            for line in wrap_text(str(raw_line), self.WIDTH - self.MARGIN * 2, measure):
                if y >= self.HEIGHT - self.LINE_HEIGHT:
                    break
                draw.text((self.MARGIN, y), line, font=font, fill=0)
                y += self.LINE_HEIGHT
            if y >= self.HEIGHT - self.LINE_HEIGHT:
                break
        self._show_image(image)

    @staticmethod
    def _default_avatar_dir() -> Path:
        roots = Path(__file__).resolve().parents
        for root in (roots[2], roots[3]):
            candidate = root / "assets" / "avatars"
            if candidate.is_dir():
                return candidate
        return roots[2] / "assets" / "avatars"

    def _load_avatar(self, directory_id: int) -> object | None:
        if directory_id not in self._avatars:
            path = self.avatar_dir / f"{directory_id:04}.png"
            try:
                avatar = self._image_module.open(path).convert("RGBA")
                avatar.thumbnail((48, 48))
                self._avatars[directory_id] = avatar
            except (OSError, ValueError):
                self._avatars[directory_id] = None
        return self._avatars[directory_id]

    def _load_font(self) -> object:
        try:
            return self._font_module.truetype(self.font_path, self.FONT_SIZE)
        except OSError:
            return self._font_module.load_default()

    @staticmethod
    def _text_width(draw: object, text: str, font: object) -> int:
        left, _, right, _ = draw.textbbox((0, 0), text, font=font)
        return right - left

    def _show_image(self, image: object) -> None:
        if self.rotation:
            image = image.rotate(self.rotation, expand=False)
        self.device.image = image
        self.device.draw = self._draw_module.Draw(image)
        self.device.show()

    def close(self) -> None:
        self.device.close()
