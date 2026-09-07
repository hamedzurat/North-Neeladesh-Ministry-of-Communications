from __future__ import annotations


class Tm1637Display:
    def __init__(self, clk: int = 27, dio: int = 17, brightness: int = 3) -> None:
        import tm1637

        self.device = tm1637.TM1637(clk=clk, dio=dio)
        self.device.set_brightness(brightness)

    def show(self, text: str) -> None:
        self.device.print(text[:4])

    def clear(self) -> None:
        self.device.clear()

    def close(self) -> None:
        self.device.close()
