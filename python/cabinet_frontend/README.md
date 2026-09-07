# Cabinet Frontend

This package is the Raspberry Pi Cabinet Frontend. It is a hardware I/O client,
not a second game core: the Rust backend remains authoritative for routing,
calls, story state, and service rules.

## Local dummy mode

The default mode never imports GPIO, SPI, I2C, or `/dev/leds0` libraries:

```sh
uv run --directory python/cabinet_frontend \
  python -m unittest discover -s hardware_frontend/tests -t . -v
NN_HARDWARE_MODE=dummy uv run --directory python/cabinet_frontend \
  python -m hardware_frontend --help
```

## Physical hardware smoke test

Run this directly on the Pi after setup. It does not connect to the game
backend. It walks the eight WS2812 pixels, writes test values to the TM1637,
renders a wrapped e-paper test page, scans MCP pairs, and listens for rotary
encoder movement. `--real` is intentionally required.

```sh
uv run --directory /home/taki/Desktop/nn-hardware-frontend \
  python -m hardware_frontend.hardware_smoke --real
```

The command clears outputs and closes devices even if interrupted. Do not run
it while the systemd frontend service is active.

## Move to the Pi

From the repository root:

```sh
PI_HOST=taki@192.168.1.11 ./scripts/deploy_hardware_frontend.sh
ssh taki@192.168.1.11 /home/taki/Desktop/nn-hardware-frontend/scripts/setup_hardware_frontend_pi.sh
```

The setup script installs the locked project and its stable dependencies into
`/home/taki/venv` with `uv sync`, grants the `gpio` group access to `/dev/leds0`,
and installs an enabled but stopped systemd service. It does not start hardware
automatically.

Start and inspect it on the Pi:

```sh
sudo systemctl start north-neeladesh-hardware-frontend.service
journalctl -u north-neeladesh-hardware-frontend.service -f
```

The frontend defaults to the authoritative backend at `127.0.0.1:7878`. If the
backend runs on the arcade host or laptop instead, pass its address during
setup, for example:

```sh
ssh taki@192.168.1.11 \
  'NN_BACKEND_ADDRESS=192.168.1.20:7878 /home/taki/Desktop/nn-hardware-frontend/scripts/setup_hardware_frontend_pi.sh'
```

## Current physical mapping

- WS2812: GPIO 12, 8 pixels, `/dev/leds0`.
- TM1637: CLK GPIO 27, DIO GPIO 17.
- MCP23017: I2C bus 1, address `0x20`.
- E-paper control: MCP pins 8, 9, 10; SPI bus 0/device 0 at 10 MHz.
- Rotary encoder: MCP pins 11 and 12.
- Pair detector: MCP pins 0 through 7.
- Keyboard fallback: set `NN_KEYBOARD_CONTROLS=1`; `p` toggles PTT, `1`/`2`/`3`
  toggle Police/EMS/Fire, `q`/`w` toggle Tap 1/2, and `x` clears all controls.
  Press `d` to edit Directory Terminal digits, use `[` and `]` to select a
  digit, and press `0`-`9` to set it.

The initial pair mapping treats MCP pins 0 through 7 as `subscriber_0`
through `subscriber_7`. The rotary mapper emits one crank timestamp per 16
detents, matching the current encoder calibration. Change
`HardwareConfig.pin_to_port` before deployment
when the physical cabinet wiring is finalized. Missing lamps, buttons, audio,
and printer hardware remain STDOUT components until their drivers exist. Audio
transport is intentionally left to `crates/voice-daemon`.
