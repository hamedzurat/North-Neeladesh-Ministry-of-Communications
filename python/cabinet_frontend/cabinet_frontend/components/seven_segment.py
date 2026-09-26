from __future__ import annotations

import threading
import time


class Tm1637Display:
    def __init__(self, clk: int = 27, dio: int = 17, brightness: int = 3) -> None:
        import tm1637

        self.device = tm1637.TM1637(clk=clk, dio=dio)
        self.device.set_brightness(brightness)
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._wake = threading.Event()
        self._target = "0000"
        self._displayed = "0000"
        self._colon_visible = True
        self._thread = threading.Thread(
            target=self._animation_loop,
            name="tm1637-animation",
            daemon=True,
        )
        self._thread.start()

    def show(self, text: str) -> None:
        with self._lock:
            self._target = text[:4]
        self._wake.set()

    def _print(self, text: str, colon: bool = False) -> None:
        if colon:
            text = f"{text[:2]}:{text[2:]}"
        self.device.print(text)

    def _render(self) -> None:
        if not self._displayed:
            self.device.clear()
            return
        self._print(self._displayed, self._colon_visible)

    def _startup(self) -> None:
        for frame in ("8888", "0000"):
            with self._lock:
                self._print(frame)
            if self._stop.wait(0.35):
                return

        message = "    NEELADESH    "
        for index in range(len(message) - 3):
            with self._lock:
                self._print(message[index : index + 4])
            if self._stop.wait(0.22):
                return

        with self._lock:
            self._displayed = "0000"

    def _roll_digits(self) -> bool:
        with self._lock:
            target = self._target
            current = self._displayed

        if target == current:
            return False
        if not current:
            current = "0000"

        position = next(
            (index for index in range(3, -1, -1) if current[index] != target[index]),
            None,
        )
        if position is None:
            return False

        next_frame = list(current)
        old_digit = int(current[position])
        new_digit = int(target[position])
        if old_digit > new_digit:
            next_frame[position] = str(old_digit - 1)
        else:
            next_frame[position] = str((old_digit + 1) % 10)

        with self._lock:
            self._displayed = "".join(next_frame)
            self._render()
        return True

    def _animation_loop(self) -> None:
        self._startup()
        next_colon = time.monotonic() + 0.5

        while not self._stop.is_set():
            now = time.monotonic()
            if self._roll_digits():
                self._stop.wait(0.07)
                continue

            with self._lock:
                if now >= next_colon:
                    self._colon_visible = not self._colon_visible
                    self._render()
                    next_colon = now + 0.5

            timeout = next_colon - time.monotonic()
            self._wake.wait(max(0.01, timeout))
            self._wake.clear()

    def clear(self) -> None:
        with self._lock:
            self._target = ""
            self._displayed = ""
            self.device.clear()

    def close(self) -> None:
        self._stop.set()
        self._wake.set()
        self._thread.join()
        self.device.close()
