"""Runtime entry point for the Raspberry Pi Cabinet Frontend."""

from __future__ import annotations

import argparse
import time
from collections.abc import Callable
from typing import Any

from .components.factory import ComponentBundle, build_dummy_components, build_real_components
from .config import HardwareConfig
from .input_mapper import InputSource, PhysicalInputSource
from .output_mapper import OutputMapper
from .protocol import BackendClient
from .state import input_message


class HardwareFrontend:
    def __init__(
        self,
        client: Any,
        input_source: InputSource,
        output_mapper: OutputMapper,
        firmware_version: str = "north-neeladesh-pi/0.1.0",
        extra_closers: list[object] | None = None,
    ) -> None:
        self.client = client
        self.input_source = input_source
        self.output_mapper = output_mapper
        self.firmware_version = firmware_version
        self.extra_closers = extra_closers or []
        self.input_sequence = 0
        self.state_revision = 0

    def step(self, now: float | None = None) -> dict[str, Any]:
        self.input_sequence += 1
        physical = self.input_source.poll(now)
        message = input_message(
            physical,
            self.input_sequence,
            self.state_revision,
            self.firmware_version,
            [
                *getattr(self.input_source, "faults", []),
                *getattr(self.output_mapper, "faults", []),
            ],
        )
        response = self.client.exchange(message)
        self.state_revision = int(response.get("state_revision", self.state_revision))
        self.output_mapper.apply(dict(response.get("output", {})))
        return response

    def close(self) -> None:
        self.input_source.close()
        self.output_mapper.close()
        for resource in self.extra_closers:
            close = getattr(resource, "close", None)
            if close is not None:
                close()
            else:
                deinit = getattr(resource, "deinit", None)
                if deinit is not None:
                    deinit()
        self.client.close()


def create_frontend(
    config: HardwareConfig,
    client: Any,
    output: Any = None,
) -> HardwareFrontend:
    components: ComponentBundle
    if config.mode == "real":
        components = build_real_components(config)
    else:
        components = build_dummy_components(output)
    input_source = PhysicalInputSource(
        components.rotary,
        components.scanner,
        config.pin_to_port,
        config.directory_digits,
        config.pair_scan_interval,
        controls=components.controls,
        crank_detents_per_rotation=config.crank_detents_per_rotation,
    )
    mapper = OutputMapper(
        components.line_lamps,
        components.seven_segment,
        components.epaper,
        components.printer,
        components.audio,
        epaper_page_interval=config.epaper_page_interval,
    )
    return HardwareFrontend(client, input_source, mapper, extra_closers=components.extra_closers)


def run_forever(
    config: HardwareConfig,
    client_factory: Callable[[], Any] | None = None,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    """Reconnect until interrupted; hardware is never created in dummy mode here."""
    factory = client_factory or (lambda: BackendClient.connect(config.backend_address))
    while True:
        client = None
        frontend = None
        try:
            client = factory()
            frontend = create_frontend(config, client)
            while True:
                frontend.step()
                sleep(config.poll_interval)
        except KeyboardInterrupt:
            if frontend is not None:
                frontend.close()
            elif client is not None:
                client.close()
            return
        except (ConnectionError, OSError, RuntimeError, ValueError) as error:
            print(f"CABINET FRONTEND OFFLINE // {error}")
            if frontend is not None:
                frontend.close()
            elif client is not None:
                client.close()
            sleep(1.0)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("dummy", "real"), default=None)
    parser.add_argument("--backend-address", default=None, metavar="HOST:PORT")
    args = parser.parse_args()
    config = HardwareConfig.from_environment()
    if args.mode is not None or args.backend_address is not None:
        address = args.backend_address or f"{config.backend_address[0]}:{config.backend_address[1]}"
        host, _, port = address.rpartition(":")
        config = HardwareConfig(
            **{
                **config.__dict__,
                "mode": args.mode or config.mode,
                "backend_address": (host, int(port)),
            }
        )
    run_forever(config)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
