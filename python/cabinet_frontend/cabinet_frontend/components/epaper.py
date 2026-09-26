from __future__ import annotations

from collections.abc import Callable, Sequence
from pathlib import Path

from ..generated_directory import generated_directory_page, unknown_id


def wrap_text(text: str, max_width: int, measure: Callable[[str], int]) -> list[str]:
    """Wrap text at word boundaries without splitting words into letters."""
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
                result.append(word)
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
    FONT_SIZE = 13
    LINE_HEIGHT = 13
    FIELD_SPACING = 4
    AVATAR_SIZE = 96

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
        self.font_path = font_path or "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
        self.font_dir = self._default_font_dir()
        self.avatar_dir = Path(avatar_dir) if avatar_dir else self._default_avatar_dir()
        self._avatars: dict[int, object | None] = {}

    def show_directory(self, pages: Sequence[dict[str, object]], page_index: int = 0) -> None:
        image = self._image_module.new("1", (self.WIDTH, self.HEIGHT), 255)
        draw = self._draw_module.Draw(image)
        font = self._load_font(weight=400)
        header_font = self._load_font(weight=600)
        page_count = len(pages)
        if not page_count:
            draw.text((self.MARGIN, self.MARGIN), "NO DIRECTORY DATA", font=font, fill=0)
            self._show_image(image)
            return

        selected_index = max(0, min(page_index, page_count - 1))
        page = pages[selected_index]
        directory_id = page.get("directory_id")
        lines = page.get("lines", [])
        if directory_id is None:
            requested_id = unknown_id(lines)
            generated = generated_directory_page(requested_id) if requested_id is not None else None
            if generated is not None:
                directory_id = generated["directory_id"]
                lines = generated["lines"]
        if isinstance(directory_id, int):
            id_font = self._load_font(16, weight=600)
            draw.text(
                (self.MARGIN, self.MARGIN),
                f"ID // {directory_id:04}",
                font=id_font,
                fill=0,
            )
            avatar = self._load_avatar(directory_id)
            if avatar is not None:
                image.paste(
                    avatar,
                    (self.WIDTH - self.MARGIN - self.AVATAR_SIZE, 0),
                    avatar,
                )
            y = 32
        else:
            large_font = self._load_font(22, weight=700)
            draw.text((self.MARGIN, 25), "ID NOT FOUND", font=large_font, fill=0)
            requested_id = str(lines[0]) if lines else "UNKNOWN ID"
            draw.text((self.MARGIN, 58), requested_id, font=self._load_font(16, weight=600), fill=0)
            lines = []
            y = 75
        if lines and str(lines[0]).upper().startswith("SUBSCRIBER ID "):
            lines = lines[1:]
        for raw_line in lines:
            display_line = str(raw_line)
            available_width = (
                self.WIDTH - self.MARGIN * 2 - self.AVATAR_SIZE - 4
                if isinstance(directory_id, int) and y < self.AVATAR_SIZE
                else self.WIDTH - self.MARGIN * 2
            )
            measure = lambda value: self._text_width(draw, value, font)
            separator = " // "
            if separator in display_line:
                header, value = display_line.split(separator, 1)
                header_text = header + separator
                header_width = self._text_width(draw, header_text, header_font)
                if header.upper() == "NAME":
                    if y < self.HEIGHT - self.LINE_HEIGHT:
                        draw.text((self.MARGIN, y), header_text, font=header_font, fill=0)
                        y += self.LINE_HEIGHT
                    for line in wrap_text(value, available_width, measure):
                        if y >= self.HEIGHT - self.LINE_HEIGHT:
                            break
                        draw.text((self.MARGIN, y), line, font=font, fill=0)
                        y += self.LINE_HEIGHT
                    y += self.FIELD_SPACING
                    if y >= self.HEIGHT - self.LINE_HEIGHT:
                        break
                    continue
                if (
                    isinstance(directory_id, int)
                    and y < self.AVATAR_SIZE
                    and header_width >= available_width
                ):
                    y = self.AVATAR_SIZE + self.FIELD_SPACING
                    available_width = self.WIDTH - self.MARGIN * 2
                if header_width >= available_width:
                    if y < self.HEIGHT - self.LINE_HEIGHT:
                        draw.text((self.MARGIN, y), header_text, font=header_font, fill=0)
                        y += self.LINE_HEIGHT
                    value_width = self.WIDTH - self.MARGIN * 2
                    value_lines = wrap_text(value, value_width, measure)
                    for line in value_lines:
                        if y >= self.HEIGHT - self.LINE_HEIGHT:
                            break
                        draw.text((self.MARGIN, y), line, font=font, fill=0)
                        y += self.LINE_HEIGHT
                else:
                    value_lines = wrap_text(
                        value,
                        max(1, available_width - header_width),
                        measure,
                    ) or [""]
                    first_line = value_lines.pop(0)
                    if y < self.HEIGHT - self.LINE_HEIGHT:
                        draw.text((self.MARGIN, y), header_text, font=header_font, fill=0)
                        draw.text(
                            (self.MARGIN + header_width, y),
                            first_line,
                            font=font,
                            fill=0,
                        )
                        y += self.LINE_HEIGHT
                    for line in value_lines:
                        if y >= self.HEIGHT - self.LINE_HEIGHT:
                            break
                        draw.text((self.MARGIN, y), line, font=font, fill=0)
                        y += self.LINE_HEIGHT
            else:
                for line in wrap_text(display_line, available_width, measure):
                    if y >= self.HEIGHT - self.LINE_HEIGHT:
                        break
                    draw.text((self.MARGIN, y), line, font=font, fill=0)
                    y += self.LINE_HEIGHT
            y += self.FIELD_SPACING
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

    @staticmethod
    def _default_font_dir() -> Path:
        roots = Path(__file__).resolve().parents
        for root in (roots[2], roots[3]):
            candidate = root / "assets" / "fonts"
            if candidate.is_dir():
                return candidate
        return roots[2] / "assets" / "fonts"

    def _load_avatar(self, directory_id: int) -> object | None:
        if directory_id not in self._avatars:
            path = self.avatar_dir / f"{directory_id:04}.png"
            if not path.is_file() and directory_id not in range(1021, 1033):
                path = self.avatar_dir / f"{directory_id % 256:04}.png"
            try:
                avatar = self._image_module.open(path).convert("RGBA")
                avatar.thumbnail((self.AVATAR_SIZE, self.AVATAR_SIZE))
                self._avatars[directory_id] = avatar
            except (OSError, ValueError):
                self._avatars[directory_id] = None
        return self._avatars[directory_id]

    def _load_font(self, size: int = FONT_SIZE, weight: int = 400) -> object:
        try:
            weight_name = {400: "Regular", 600: "SemiBold", 700: "Bold"}[weight]
            font_path = self.font_dir / f"Inter-{weight_name}.ttf"
            if not font_path.is_file():
                font_path = Path(self.font_path)
            return self._font_module.truetype(font_path, size)
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
