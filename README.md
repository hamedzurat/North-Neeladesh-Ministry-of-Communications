# North Neeladesh

Local development instructions for the hardware-first demo. The Rust backend
owns the game state. The Odin frontend is the GUI simulator for the telephone
cabinet. Odin and the Python Cabinet Frontend are alternative gameplay clients:
use Odin to test locally, and use the Cabinet Frontend as the arcade client.
Only one gameplay client should be connected to a backend at a time.

## Prerequisites

Install or make available:

- Rust and Cargo
- `just`
- Odin at `/bin/odin`
- Python, `uv`, and the dependencies in `python/`
- A graphical desktop session for the Odin window

Run all commands from the repository root.

Gameplay tuning and Subscriber profiles live in [`exchange.toml`](exchange.toml).
Set `NN_EXCHANGE_CONFIG` to use another configuration file.
Shared physical game rules are documented in [`docs/mechanics.md`](docs/mechanics.md).
The full player-facing mechanic and story guide is in [`docs/gameplay.md`](docs/gameplay.md).

## First Check

Run the automated checks before starting the GUI:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
just protocol-harness
just frontend-check
just voice-checks
just voice-preflight
uv run --directory python/cabinet_frontend \
  python -m unittest discover -s cabinet_frontend/tests -t . -v
```

The GUI build can be checked separately:

```sh
just frontend-build
```

## GUI Test Run

This is the complete local Odin setup, including real voice transport. Use three
terminals. Do not start the Cabinet Frontend in this run; it is the alternative
arcade client described later.

### Terminal 1: debug backend

```sh
just backend-debug
```

The backend uses the CPU-only PocketTTS worker:

```sh
just backend-debug
```

This is the authoritative game backend. It listens on:

- Game protocol: `127.0.0.1:7878`
- Voice protocol: `127.0.0.1:7879`
- Debug command protocol: `127.0.0.1:7880`

Do not run another backend on these ports.

### Terminal 2: debug dashboard

```sh
just debug-surface
```

Open <http://127.0.0.1:7881> in a browser. The dashboard is an observer and
development-control surface, not the gameplay UI. It shows the authoritative
Run, Shift, Calls, exact frontend wire state, Cabinet status,
diagnostics, story state machines, LLM prompts/responses, and retained voice evidence.

For the story-test backend, use `just story-debug-surface`; it connects to the
story backend's debug port `127.0.0.1:7882` instead of the normal backend port.

Before playing:

1. Confirm the dashboard says `BACKEND CONNECTED`.
2. Confirm the exchange starts with active Calls.
3. Click `Reset run` once so the test starts from a known state.

During the test, use the dashboard to verify that Odin input changes the
authoritative state. Use debug controls only for isolated backend checks, not as
a substitute for the physical routing flow.

At the end, click `Reset run` and verify that Calls, counters, elapsed time,
printer output, and retained voice evidence return to the
start state.

### Terminal 3: Odin gameplay GUI

The Odin frontend now owns microphone capture, voice UDP control/status, and
speaker playback. The backend still runs STT, dialogue, and TTS.

Before the first real voice run, provision the local model assets:

```sh
just voice-setup
just voice-preflight
```

`voice-setup` also downloads the PocketTTS voice prompt into the ignored
`python/.models/` directory.

```sh
just frontend
```

This builds the frontend and opens a borderless full-screen window connected to
`127.0.0.1:7878`. Close the window to stop it. With an Operator cord
connected, hold `PTT / OPERATOR` to test the complete voice path. The default
voice endpoint is `127.0.0.1:7879`; override it with
`NN_VOICE_BACKEND_ADDRESS`. Odin uses a native PipeWire capture client and
keeps that stream alive for the lifetime of the frontend.

For wire-level troubleshooting instead:

```sh
just frontend-trace
```

The frontend stays open and displays `BACKEND OFFLINE // RETRYING` if the
backend is unavailable. Start Terminal 1 first, or wait for reconnection.

## Odin Controls

The logical cabinet is shown on screen. Use the visible labels rather than
fixed screen coordinates because the window is letterboxed on different
monitors.

- Drag between two jacks to create a cord.
- Right-click either end of a cord to remove it.
- Hold an action button to send a held control; release the mouse to release it.
- Click `+` or `-` above or below a Directory digit.
- Scroll over `SCROLL TO CRANK` until the crank flashes.
- Drag on `TUNING 1` and `TUNING 2` to set the two tuning values.

The action buttons are:

- `PTT / OPERATOR`
- `POLICE`
- `EMS`
- `TAP BRIDGE LISTEN`

The initial Directory value is `0001`. The live hardware story uses subscriber
lines `0` through `5`, keeps two Calls active, and chooses new caller/callee
pairs randomly after each completed Call.

## Manual GUI Test

Use the four-terminal setup above. Reset from the debug dashboard rather than
restarting processes. If the run gets into an unexpected state, click
`Reset run`, wait for the dashboard to show `run_start`, and clear any cords in
Odin before continuing.

### Live hardware call loop

1. Wait for two caller lamps between lines `0` and `7` to light.
2. Connect either caller to `OPERATOR` and hold `PTT / OPERATOR`. The caller
   names the destination; set the Directory display to that destination line
   (`0000` through `0005`).
3. Leave the caller on `OPERATOR`, connect the requested callee to `RING
   GENERATOR`, and crank until the callee lamp lights.
4. Disconnect the Ring Generator, connect the caller directly to the callee, and
   leave both lamps lit for three seconds.
5. The completed call is replaced automatically. Repeat with either active
   caller.

The backend rejects direct routing before ringing and rejects Directory values
outside the requested `0` through `5` line. The physical Cabinet Frontend uses
the same snapshots and input sequence as the Odin simulator.

## Arcade Cabinet Client

The Cabinet Frontend is the intended arcade client. It is not an observer that
should run beside Odin. The normal split is:

- Odin GUI: local development and gameplay testing.
- Raspberry Pi Cabinet Frontend: the physical arcade client.
- Rust backend: one authoritative game process used by either client.

Play the same live call loop from the Cabinet controls instead of starting Odin. Do not connect
Odin and the Cabinet Frontend to the same backend at once; both send ordered
input snapshots and would compete over topology and state revision.

For a networked arcade setup, run the backend on the laptop/arcade host with a
LAN-reachable address, then configure the Pi to use that host:

See [`docs/deploy-cabinet-wifi.md`](docs/deploy-cabinet-wifi.md) for the full
deployment and Wi‑Fi troubleshooting procedure.

```sh
# On the laptop or arcade host. Restrict these ports with the local firewall.
just backend-debug address=0.0.0.0:7878 voice_address=0.0.0.0:7879

# On the Pi, deploy the combined Python Cabinet Frontend bundle.
export PI_HOST=taki@192.168.1.34
BACKEND_HOST=192.168.1.8 ./scripts/deploy_cabinet_frontend.sh
```

Replace `192.168.1.8` with the laptop's Wi-Fi/LAN address. The deploy script
prints the exact Pi setup command; `NN_BACKEND_HOST` configures both game TCP
(`7878`) and voice UDP (`7879`), so no second voice IP setting is needed. The
backend must listen on LAN interfaces, not only localhost. Start the installed
service on the Pi:

```sh
sudo systemctl start north-neeladesh-cabinet-frontend.service
journalctl -u north-neeladesh-cabinet-frontend.service -f
```

From the development machine, inspect the frontend service on the Pi at
`taki@192.168.1.34` with `./scripts/status_pi.sh`.

The Python voice relay runs inside the Cabinet Frontend process. PTT, Police,
and EMS edges start capture through the same frontend lifecycle; the backend
remains authoritative for STT, dialogue, classification, and TTS.

### Cabinet frontend

The Cabinet Frontend runs against the physical Raspberry Pi hardware. It reads
the patch panel, switches, rotary encoder, and Directory buttons, then sends
the resulting input snapshot to the backend. It drives the lamps, TM1637,
e-paper display, and `/dev/usb/lp0` printer. Voice capture, transport, and
playback run in the same process.

Run the frontend tests with:

Run the Cabinet tests independently with:

```sh
uv run --directory python/cabinet_frontend \
  python -m unittest discover -s cabinet_frontend/tests -t . -v
```

## Voice Diagnostics

Voice failure must produce a diagnostic; it must not create a fake Routing or
Story event. For the real microphone/speaker path, run `just frontend` as
described in the GUI test run. Odin uses a native PipeWire capture stream and
Raylib for playback; the Python Cabinet Frontend remains the equivalent relay
for Raspberry Pi hardware.

## Debug Surface Reference

The debug dashboard is part of the recommended test setup. It uses
`127.0.0.1:7880` for backend commands and listens at `127.0.0.1:7881`; Odin and
the Cabinet still use the normal game protocol at `127.0.0.1:7878`.

Use these dashboard controls only for targeted checks:

- `Advance time`: verify that the backend-owned accelerated clock changes.
- `Inject call`: verify that the backend records an additional Call.
- `Force event`: verify an authored Story Event transition.
- `Select path`: request an authored frontier path.
- `Toggle godmode`: temporarily bypass restrictions for development diagnosis.
- `Toggle bypass`: temporarily permit otherwise restricted debug setup.
- `Reset run`: clear all authoritative state and retained debug evidence.

For the normal GUI proof, leave godmode and bypass disabled and do not inject,
force, or select anything. The dashboard should passively show the state
changes caused by Odin. Its `Frontend wire state` panel is especially useful:
the input JSON proves what Odin sent, while the output JSON proves what the
backend returned.

## What Should Pass Now

The following are implemented and covered by automated tests:

- backend-authoritative snapshots over TCP/CBOR;
- Directory-gated routing;
- pre-ring rejection with `ring_generator_required`;
- Ring Generator plus crank gating;
- two simultaneous random Calls on subscriber lines 0 through 5;
- caller-to-Operator voice context with the requested callee;
- three-second connected lamp windows and automatic Call replacement;
- Odin rendering of backend output;
- Cabinet hardware input mapping, output mapping, and printer reset reconciliation.

The legacy authored graph remains available for backend regression tests, but it
is no longer used by the live hardware server. The live loop has no terminal
Ending; it continues generating two Calls for the duration of the shift.

## Related Documentation

- [`docs/frontend-protocol.md`](docs/frontend-protocol.md): wire contract and
  manual verification notes.
- [`docs/current-status-hardware-demo.md`](docs/current-status-hardware-demo.md):
  current implementation status and remaining gaps.
- [`python/cabinet_frontend/README.md`](python/cabinet_frontend/README.md): Pi
  deployment and physical hardware smoke test.
