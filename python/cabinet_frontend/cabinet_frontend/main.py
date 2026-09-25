"""Runtime entry point for the Raspberry Pi Cabinet Frontend."""

from __future__ import annotations

import argparse
import queue
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
        background_outputs: bool = False,
        background_transport: bool = False,
    ) -> None:
        self.client = client
        self.input_source = input_source
        self.output_mapper = output_mapper
        self.firmware_version = firmware_version
        self.extra_closers = extra_closers or []
        self.input_sequence = 0
        self.state_revision = 0
        self.diagnostics = ChangeLogger(status_sink)
        self._output_queue: queue.Queue[dict[str, Any] | None] | None = None
        self._output_thread: threading.Thread | None = None
        self._transport_queue: queue.Queue[dict[str, Any] | None] | None = None
        self._transport_thread: threading.Thread | None = None
        self._transport_lock = threading.Lock()
        self._sent_at: dict[int, float] = {}
        self._last_response: dict[str, Any] = {
            "accepted": True,
            "state_revision": 0,
            "output": {},
        }
        if background_outputs:
            self._output_queue = queue.Queue(maxsize=1)
            self._output_thread = threading.Thread(
                target=self._output_loop,
                name="cabinet-output-worker",
                daemon=True,
            )
            self._output_thread.start()
        if background_transport:
            self._transport_queue = queue.Queue(maxsize=1)
            self._transport_thread = threading.Thread(
                target=self._transport_loop,
                name="cabinet-game-transport",
                daemon=True,
            )
            self._transport_thread.start()

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
        if self._transport_queue is not None:
            try:
                self._transport_queue.get_nowait()
            except queue.Empty:
                pass
            self._transport_queue.put_nowait(message)
            with self._transport_lock:
                return dict(self._last_response)
        response = self.client.exchange(message)
        self._handle_response(response)
        return response

    def _handle_response(self, response: dict[str, Any]) -> None:
        input_sequence = response.get("input_sequence")
        if isinstance(input_sequence, int):
            sent_at = self._sent_at.pop(input_sequence, None)
            if sent_at is not None and (
                response.get("accepted") is False or response.get("error") is not None
            ):
                latency_us = int((time.monotonic() - sent_at) * 1_000_000)
                self.diagnostics.emit(
                    "input_ack",
                    (input_sequence, latency_us),
                    f"BACKEND // input_ack sequence={input_sequence} latency_us={latency_us}",
                )
        with self._transport_lock:
            self.state_revision = int(response.get("state_revision", self.state_revision))
            self._last_response = dict(response)
        if response.get("accepted") is not True or response.get("error") is not None:
            self.diagnostics.emit(
                "backend_acceptance",
                (response.get("accepted"), response.get("error")),
                _backend_result_message(response),
            )
        output = dict(response.get("output", {}))
        if self._output_queue is None:
            self.output_mapper.apply(output)
        else:
            try:
                self._output_queue.get_nowait()
            except queue.Empty:
                pass
            self._output_queue.put_nowait(output)

    def _transport_loop(self) -> None:
        assert self._transport_queue is not None
        while True:
            message = self._transport_queue.get()
            if message is None:
                return
            try:
                # The snapshot may have waited in the coalescing queue while
                # an earlier request advanced the backend revision. Refresh
                # the optimistic-concurrency token immediately before send.
                with self._transport_lock:
                    message["expected_state_revision"] = self.state_revision
                self._sent_at[int(message["input_sequence"])] = time.monotonic()
                response = self.client.exchange(message)
                self._handle_response(response)
            except Exception as error:  # noqa: BLE001 - main loop remains responsive
                self.diagnostics.emit(
                    "transport_failure",
                    (type(error).__name__, str(error)),
                    f"FRONTEND // transport failed error={type(error).__name__}: {error}",
                )

    def _output_loop(self) -> None:
        assert self._output_queue is not None
        while True:
            output = self._output_queue.get()
            if output is None:
                return
            try:
                self.output_mapper.apply(output)
            except Exception as error:  # noqa: BLE001 - report hardware failures
                self.diagnostics.emit(
                    "output_failure",
                    (type(error).__name__, str(error)),
                    f"FRONTEND // output failed error={type(error).__name__}: {error}",
                )

    def close(self) -> None:
        if self._transport_queue is not None:
            self._transport_queue.put(None)
        if self._transport_thread is not None:
            self._transport_thread.join(timeout=2.0)
        if self._output_queue is not None:
            self._output_queue.put(None)
        if self._output_thread is not None:
            self._output_thread.join(timeout=2.0)
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
    input_sequence: int = 0,
    state_revision: int = 0,
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
        background_scanning=True,
        control_poll_interval=config.control_poll_interval,
        pair_line_interval=config.pair_line_interval,
    )
    mapper = OutputMapper(
        components.line_lamps,
        components.seven_segment,
        components.epaper,
        components.printer,
        line_lamp_count=config.line_lamp_count,
        epaper_page_interval=config.epaper_page_interval,
        epaper_update_delay=config.epaper_update_delay,
        local_clock_display=config.local_clock_display,
    )
    frontend = HardwareFrontend(
        client,
        input_source,
        mapper,
        extra_closers=components.extra_closers,
        background_outputs=True,
        background_transport=True,
    )
    frontend.input_sequence = input_sequence
    frontend.state_revision = state_revision
    return frontend


def run_forever(
    config: HardwareConfig,
    client_factory: Callable[[], Any] | None = None,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    """Reconnect until interrupted."""
    factory = client_factory or (lambda: BackendClient.connect(config.backend_address))
    diagnostics = ChangeLogger(print)
    input_sequence = 0
    state_revision = 0
    voice_stop = threading.Event()
    voice_thread: threading.Thread | None = None
    while True:
        client = None
        frontend = None
        try:
            client = factory()
            diagnostics.emit(
                "transport",
                "connected",
                "CABINET FRONTEND // backend connected",
            )
            if voice_thread is None:
                voice_thread = threading.Thread(
                    target=run_embedded,
                    args=(voice_stop, config.voice_upload_address, config.voice_playback_gain),
                    name="cabinet-voice-relay",
                    daemon=True,
                )
                voice_thread.start()
            frontend = create_frontend(config, client, input_sequence, state_revision)
            while True:
                frontend.step()
                # Sample physical controls faster than the normal backend cadence.
                sleep(min(config.poll_interval, 0.02))
        except KeyboardInterrupt:
            voice_stop.set()
            if voice_thread is not None:
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
            if frontend is not None:
                input_sequence = frontend.input_sequence
                state_revision = frontend.state_revision
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
