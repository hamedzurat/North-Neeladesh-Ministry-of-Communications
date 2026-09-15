from __future__ import annotations


class DevicePrinter:
    def __init__(self, device: str = "/dev/usb/lp0") -> None:
        self.device = device

    def write(self, text: str) -> None:
        with open(self.device, "ab", buffering=0) as printer:
            printer.write(text.encode())

    def close(self) -> None:
        return None
