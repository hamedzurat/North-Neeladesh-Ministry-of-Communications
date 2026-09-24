"""Runtime entry point for the Raspberry Pi Cabinet Frontend."""

from __future__ import annotations

import argparse
import threading
import time
from collections.abc import Callable
from typing import Any

from .components.factory import ComponentBundle, build_real_components
from .config import HardwareConfig, parse_backend_address
from .diagnostics import ChangeLogger
from .input_mapper import InputSource, PhysicalInputSource
from .output_mapper import OutputMapper
from .protocol import BackendClient
from .state import input_message
from .voice_relay import run_embedded


class HardwareFrontend:
    def __init__(
        self,
        client: Any,
        input_source: InputSource,
        output_mapper: OutputMapper,
        firmware_version: str = "north-neeladesh-pi/0.1.0",
        extra_closers: list[object] | None = None,
        status_sink: Callable[[str], None] | None = print,
    ) -> None:
        self.client = client
        self.input_source = input_source
        self.output_mapper = output_mapper
        self.firmware_version = firmware_version
        self.extra_closers = extra_closers or []
        self.input_sequence = 0
        self.state_revision = 0
        self.diagnostics = ChangeLogger(status_sink)

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
        self.diagnostics.emit(
            "backend_acceptance",
            (response.get("accepted"), response.get("error")),
            _backend_result_message(response),
        )
        self.output_mapper.apply(dict(response.get("output", {})))
        return response

    def close(self) -> None:
        resources = [self.input_source, self.output_mapper, *self.extra_closers, self.client]
        for resource in resources:
            try:
                close = getattr(resource, "close", None)
                if close is not None:
                    close()
                else:
                    deinit = getattr(resource, "deinit", None)
                    if deinit is not None:
                        deinit()
            except Exception as error:  # noqa: BLE001 - lost hardware must not block restart
                print(
                    "CABINET FRONTEND // cleanup failed "
                    f"component={type(resource).__name__} "
                    f"error={type(error).__name__}: {error}",
                    flush=True,
                )


def create_frontend(
    config: HardwareConfig,
    client: Any,
) -> HardwareFrontend:
    components: ComponentBundle
    components = build_real_components(config)
    input_source = PhysicalInputSource(
        components.rotary,
        components.scanner,
        config.pin_to_port,
        config.directory_digits,
        config.pair_scan_interval,
        controls=components.controls,
        crank_detents_per_rotation=config.crank_detents_per_rotation,
        status_interval=config.input_status_interval,
        tuning=(config.tuning_coarse, config.tuning_fine),
    )
    mapper = OutputMapper(
        components.line_lamps,
        components.seven_segment,
        components.epaper,
        components.printer,
        line_lamp_count=config.line_lamp_count,
        epaper_page_interval=config.epaper_page_interval,
    )
    return HardwareFrontend(client, input_source, mapper, extra_closers=components.extra_closers)


def run_forever(
    config: HardwareConfig,
    client_factory: Callable[[], Any] | None = None,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    """Reconnect until interrupted."""
    factory = client_factory or (lambda: BackendClient.connect(config.backend_address))
    diagnostics = ChangeLogger(print)
    while True:
        client = None
        frontend = None
        voice_stop: threading.Event | None = None
        voice_thread: threading.Thread | None = None
        try:
            client = factory()
            diagnostics.emit(
                "transport",
                "connected",
                "CABINET FRONTEND // backend connected",
            )
            voice_stop = threading.Event()
            voice_thread = threading.Thread(
                target=run_embedded,
                args=(voice_stop,),
                name="cabinet-voice-relay",
                daemon=True,
            )
            voice_thread.start()
            frontend = create_frontend(config, client)
            while True:
                frontend.step()
                # Sample physical controls faster than the normal backend cadence.
                sleep(min(config.poll_interval, 0.02))
        except KeyboardInterrupt:
            if voice_stop is not None and voice_thread is not None:
                voice_stop.set()
                voice_thread.join(timeout=2.0)
            if frontend is not None:
                frontend.close()
            elif client is not None:
                client.close()
            return
        except Exception as error:  # noqa: BLE001 - restart after any device/transport failure
            diagnostics.emit(
                "transport",
                ("offline", type(error).__name__, str(error)),
                f"CABINET FRONTEND OFFLINE // {type(error).__name__}: {error}",
            )
            if voice_stop is not None and voice_thread is not None:
                voice_stop.set()
                voice_thread.join(timeout=2.0)
            if frontend is not None:
                frontend.close()
            elif client is not None:
                client.close()
            sleep(1.0)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend-address", default=None, metavar="HOST:PORT")
    args = parser.parse_args()
    config = HardwareConfig.from_environment()
    if args.backend_address is not None:
        address = args.backend_address or f"{config.backend_address[0]}:{config.backend_address[1]}"
        host, port = parse_backend_address(address)
        config = HardwareConfig(
            **{
                **config.__dict__,
                "backend_address": (host, port),
            }
        )
    run_forever(config)
    return 0


def _backend_result_message(response: dict[str, Any]) -> str:
    error = response.get("error")
    if isinstance(error, dict):
        return f"BACKEND // rejected code={error.get('code', 'unknown')} message={error.get('message', '')}"
    return f"BACKEND // accepted state_revision={response.get('state_revision', '?')}"


if __name__ == "__main__":
    raise SystemExit(main())
