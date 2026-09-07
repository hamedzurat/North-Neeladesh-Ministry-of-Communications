"""Optional terminal keyboard fallback for missing Cabinet controls."""

from __future__ import annotations

import select
import sys
import termios
import tty
from typing import ClassVar, TextIO

from ..state import HeldControls


class NoopControls:
    directory_digits: ClassVar[list[int]] = [0, 0, 0, 1]

    def poll(self) -> HeldControls:
        return HeldControls()

    def close(self) -> None:
        return None


class KeyboardControls:
    KEY_BINDINGS: ClassVar[dict[str, str]] = {
        "p": "ptt",
        "1": "police",
        "2": "ems",
        "3": "fire",
        "q": "tap_1",
        "w": "tap_2",
    }

    def __init__(self, input_stream: TextIO = sys.stdin, output: TextIO = sys.stdout) -> None:
        self.input_stream = input_stream
        self.output = output
        self.controls = HeldControls()
        self.directory_digits = [0, 0, 0, 1]
        self.selected_digit = 3
        self.editing_digits = False
        self._old_terminal: list[int] | None = None
        if input_stream.isatty():
            self._old_terminal = termios.tcgetattr(input_stream.fileno())
            tty.setcbreak(input_stream.fileno())
        print("KEYBOARD CONTROLS // p=PTT 1=POLICE 2=EMS 3=FIRE q= TAP1 w= TAP2", file=output)

    def poll(self) -> HeldControls:
        if not self.input_stream.isatty():
            return self.controls
        while select.select([self.input_stream], [], [], 0)[0]:
            key = self.input_stream.read(1).lower()
            self.handle_key(key)
        return self.controls

    def handle_key(self, key: str) -> None:
        if key == "x":
            self.controls = HeldControls()
            print("KEYBOARD CONTROLS // cleared", file=self.output)
            return
        if key == "d":
            self.editing_digits = not self.editing_digits
            print(f"KEYBOARD DIRECTORY // editing={self.editing_digits}", file=self.output)
            return
        if self.editing_digits:
            if key == "[":
                self.selected_digit = (self.selected_digit - 1) % 4
            elif key == "]":
                self.selected_digit = (self.selected_digit + 1) % 4
            elif key.isdigit():
                self.directory_digits[self.selected_digit] = int(key)
            else:
                return
            print(
                f"KEYBOARD DIRECTORY // digits={''.join(map(str, self.directory_digits))} "
                f"selected={self.selected_digit}",
                file=self.output,
            )
            return
        name = self.KEY_BINDINGS.get(key)
        if name is not None:
            value = not getattr(self.controls, name)
            setattr(self.controls, name, value)
            print(f"KEYBOARD CONTROLS // {name}={value}", file=self.output)

    def close(self) -> None:
        if self._old_terminal is not None:
            termios.tcsetattr(self.input_stream.fileno(), termios.TCSADRAIN, self._old_terminal)
