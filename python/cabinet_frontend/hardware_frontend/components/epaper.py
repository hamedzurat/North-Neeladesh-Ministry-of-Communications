from __future__ import annotations

from collections.abc import Callable, Sequence


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

    def __init__(self, mcp: object, font_path: str | None = None) -> None:
        import epaper
        from PIL import Image, ImageDraw, ImageFont

        self._image_module = Image
        self._draw_module = ImageDraw
        self._font_module = ImageFont
        self.device = epaper.EPaper(mcp)
        self.device.init()
        self.font_path = font_path or "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"

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
        draw.text(
            (self.MARGIN, self.MARGIN),
            f"PAGE {selected_index + 1}/{page_count}",
            font=font,
            fill=0,
        )
        y = self.MARGIN + self.LINE_HEIGHT + 3
        heading = str(page.get("heading", "")).upper()
        measure = lambda value: self._text_width(draw, value, font)
        for line in wrap_text(heading, self.WIDTH - self.MARGIN * 2, measure):
            if y >= self.HEIGHT - self.LINE_HEIGHT:
                break
            draw.text((self.MARGIN, y), line, font=font, fill=0)
            y += self.LINE_HEIGHT
        y += 3
        for raw_line in page.get("lines", []):
            for line in wrap_text(str(raw_line), self.WIDTH - self.MARGIN * 2, measure):
                if y >= self.HEIGHT - self.LINE_HEIGHT:
                    break
                draw.text((self.MARGIN, y), line, font=font, fill=0)
                y += self.LINE_HEIGHT
            if y >= self.HEIGHT - self.LINE_HEIGHT:
                break
        self._show_image(image)

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
        self.device.image = image
        self.device.draw = self._draw_module.Draw(image)
        self.device.show()

    def close(self) -> None:
        self.device.close()
