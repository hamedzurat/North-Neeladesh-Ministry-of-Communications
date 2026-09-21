# Cabinet Frontend

This package is the Raspberry Pi Cabinet Frontend. It is a hardware I/O client,
not a second game core: the Rust backend remains authoritative for routing,
calls, story state, and service rules.

## Tests

```sh
uv run --directory python/cabinet_frontend \
  python -m unittest discover -s cabinet_frontend/tests -t . -v
```

## Voice relay

The Pi voice relay is now implemented in `cabinet_frontend.voice_relay`. It uses
the same Python deployment as the cabinet frontend; no Rust toolchain or voice
daemon binary is required on the Pi. The setup script installs the relay as
`north-neeladesh-voice-relay.service` alongside the cabinet service.

The relay remains wire-compatible with the backend: tagged CBOR over connected
UDP for control, status, and 16 kHz input PCM, plus RTP/L16 at 24 kHz for
speaker audio. Configure `NN_VOICE_BACKEND_ADDRESS` during setup if the backend
is not on localhost.

## Physical hardware smoke test

Run this directly on the Pi after setup. It does not connect to the game
backend. It walks the sixteen WS2812 pixels, writes test values to the TM1637,
renders a wrapped e-paper test page, scans MCP pairs, and listens for rotary
encoder movement. `--real` is intentionally required.

```sh
PYTHONPATH=/home/taki/Desktop \
UV_PROJECT_ENVIRONMENT=/home/taki/venv \
uv run --directory /home/taki/Desktop/cabinet-frontend \
  python -m cabinet_frontend.hardware_smoke --real
```

The command clears outputs and closes devices even if interrupted. Do not run
it while the systemd frontend service is active.

## Move to the Pi

From the repository root:

```sh
PI_HOST=taki@192.168.1.34 ./scripts/deploy_cabinet_frontend.sh
ssh taki@192.168.1.34 /home/taki/Desktop/cabinet-frontend/scripts/setup_cabinet_frontend_pi.sh
```

The setup script installs the locked project and its stable dependencies into
`/home/taki/venv` with `uv sync`, grants the `gpio` group access to `/dev/leds0`,
and installs an enabled but stopped systemd service. It does not start hardware
automatically.

Start and inspect it on the Pi:

```sh
sudo systemctl start north-neeladesh-cabinet-frontend.service
journalctl -u north-neeladesh-cabinet-frontend.service -f
```

From the development machine, inspect both services on the Pi:

```sh
./scripts/status_pi.sh
```

This connects to `taki@192.168.1.34` over SSH and runs `systemctl` there. Set
`PI_HOST` to override the SSH target.

The frontend defaults to the authoritative backend at `127.0.0.1:7878`. If the
backend runs on the arcade host or laptop instead, pass its address during
setup, for example:

```sh
ssh taki@192.168.1.34 \
  'NN_BACKEND_ADDRESS=192.168.1.8:7878 /home/taki/Desktop/cabinet-frontend/scripts/setup_cabinet_frontend_pi.sh'
```

## Physical mapping

- MCP23017: SDA GPIO 2, SCL GPIO 3, address `0x20`; patch panel MCP pins 0 through 15.
- WS2812: DIN GPIO 12, 16 pixels, `/dev/leds0`.
- TM1637: CLK GPIO 27, DIO GPIO 17.
- MAX98357A: LRC GPIO 19, BCLK GPIO 18, DIN GPIO 21.
- E-paper: BUSY GPIO 25, RST GPIO 24, DC GPIO 23, CS GPIO 8, SCLK GPIO 11,
  SDA GPIO 10; SPI bus 0/device 0 at 10 MHz.
- Rotary encoder: S1 GPIO 14, S2 GPIO 15.
- Toggle switches: GPIO 5, 22, 9, 0.
- E-paper buttons: GPIO 4, 6, 13, 26.
- Pair detector: MCP pins 0 through 11 are `subscriber_0` through
  `subscriber_11`; pin 12 is `operator`; pin 13 is `ring_generator`; pins 14
  and 15 are
  `tap_1` and `tap_2`.
- Toggle switches GPIO 5, 22, 9, and 0 map to PTT, Police, EMS, and Tap.
  Inputs use 50 ms debounce.
- E-paper buttons GPIO 4, 6, 13, and 26 increment directory digits 1 through
  4, respectively.

The rotary mapper arms `ring_line` after one full rotation (16 detents),
matching the current encoder calibration. The line must remain connected to
the Ring Generator; the backend remains responsible for accepting the ring.
All physical pin values are defined in `cabinet_frontend/config.py`. Audio
transport runs through the bundled Python `cabinet_frontend.voice_relay` service.
