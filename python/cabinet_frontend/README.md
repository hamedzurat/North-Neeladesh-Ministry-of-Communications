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

The Pi voice relay is implemented in `cabinet_frontend.voice_relay` and runs
inside the cabinet frontend process; no Rust toolchain, voice daemon binary, or
second service is required on the Pi.

The relay keeps one bounded PipeWire capture stream open for the lifetime of the
frontend, even while the game TCP connection is unavailable. PTT, Police, and
EMS mark a capture boundary; releasing the control sends the audio collected
since that boundary. Set `NN_VOICE_CAPTURE_COMMAND` only to use a custom capture
command instead. The normal path uses a callback-based `sounddevice`/
PortAudio stream routed through PipeWire; `pw-record` remains a fallback. The
relay remains wire-compatible with the backend:
tagged CBOR over connected
UDP for control, status, and 16 kHz input PCM, plus RTP/L16 at 24 kHz for
speaker audio. Configure `NN_VOICE_BACKEND_ADDRESS` during setup if the backend
is not on localhost.

## Diagnostics

The frontend logs connection changes, backend acceptance/rejection changes,
hardware faults, calls, service calls, Tap Bridge monitoring, story progress,
voice status transitions, RTP stream changes, packet gaps, and recovery events.
Repeated polling of an unchanged state is intentionally silent. On the Pi,
these messages are available with:

```sh
journalctl -u north-neeladesh-cabinet-frontend.service -f
```

## Physical hardware smoke test

For the complete laptop-to-Pi Wi‑Fi deployment procedure, see
[`../../docs/deploy-cabinet-wifi.md`](../../docs/deploy-cabinet-wifi.md).

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
PI_HOST=taki@192.168.1.34 BACKEND_HOST=192.168.1.8 \
  ./scripts/deploy_cabinet_frontend.sh
```

The setup script creates the locked project environment and installs its stable dependencies into
`/home/taki/venv` with `uv sync`, copies the authored story audio into
`assets/stories`, grants the `gpio` group access to `/dev/leds0`, and installs
an enabled but stopped systemd service. It does not start hardware
automatically. The audio files are kept in the Pi bundle so the deployment is
self-contained and can also be used if the authoritative backend is moved to
the Pi later.

The setup also grants the service access to the `lp` group and `/dev/usb/lp0`
through udev. A new login may be required for the user-level group membership,
but the systemd service receives `lp` through `SupplementaryGroups` immediately
after it is restarted.

Start and inspect it on the Pi:

```sh
sudo systemctl start north-neeladesh-cabinet-frontend.service
journalctl -u north-neeladesh-cabinet-frontend.service -f
```

## Run manually

Stop the systemd service first, then run the guarded launcher in the foreground:

```sh
ssh -t taki@10.15.32.101 \
  'sudo systemctl stop north-neeladesh-cabinet-frontend.service'
ssh -t taki@10.15.32.101 \
  'NN_BACKEND_HOST=10.15.28.117 \
   /home/taki/Desktop/cabinet-frontend/scripts/run_cabinet_frontend_pi.sh'
```

The launcher refuses to run if systemd is still active. Press `Ctrl-C` to stop
the foreground process, then start systemd again when required.

From the development machine, inspect the frontend service on the Pi:

```sh
./scripts/status_pi.sh
```

This connects to `taki@192.168.1.34` over SSH and runs `systemctl` there. Set
`PI_HOST` to override the SSH target.

The frontend defaults to the authoritative backend at `127.0.0.1`. For a
Wi-Fi/LAN backend, set one host value during setup. It configures game TCP on
port `7878` and voice UDP on port `7879`:

```sh
ssh taki@192.168.1.34 \
  'NN_BACKEND_HOST=192.168.1.8 /home/taki/Desktop/cabinet-frontend/scripts/setup_cabinet_frontend_pi.sh'
```

Use `NN_BACKEND_ADDRESS=HOST:PORT` or `NN_VOICE_BACKEND_ADDRESS=HOST:PORT`
only when overriding the default ports independently.

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
- Patch-panel topology is rescanned every 0.5 seconds by default. Set
  `NN_PAIR_SCAN_INTERVAL` during setup to override it; once a change is found,
  the next 50 Hz frontend exchange sends it to the backend.
- E-paper buttons GPIO 4, 6, 13, and 26 increment directory digits 1 through
  4, respectively.

The rotary mapper arms `ring_line` after one full rotation (16 detents),
matching the current encoder calibration. The line must remain connected to
the Ring Generator; the backend remains responsible for accepting the ring.
All physical pin values are defined in `cabinet_frontend/config.py`. Audio
transport runs through the embedded Python `cabinet_frontend.voice_relay` thread.
