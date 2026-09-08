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
  python -m unittest discover -s hardware_frontend/tests -t . -v
```

The GUI build can be checked separately:

```sh
just frontend-build
```

## GUI Test Run

This is the complete local Odin setup, including real voice transport. Use four
terminals. Do not start the Cabinet Frontend in this run; it is the alternative
arcade client described later.

### Terminal 1: debug backend

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
Run, Shift, Calls, Story Graph, exact frontend wire state, Cabinet status,
diagnostics, and retained voice evidence.

Before playing:

1. Confirm the dashboard says `BACKEND CONNECTED`.
2. Confirm the current node is `run_start` and Directory starts at `0001`.
3. Confirm `Godmode` and `Bypass Restrictions` are disabled.
4. Click `Reset run` once so the test starts from a known state.

During the test, use the dashboard to verify that Odin input changes the
authoritative state. Do not use `Inject call`, `Force event`, `Select path`,
`Godmode`, or `Bypass Restrictions` for the normal gameplay proof. Those
controls are for isolated backend/debug checks.

At the end, click `Reset run` and verify that Calls, counters, elapsed time,
printer output, Story Graph state, and retained voice evidence return to the
start state.

### Terminal 3: voice daemon

The daemon is required for microphone capture and speaker playback. It is the
Cabinet-side audio transport; the backend still runs STT, dialogue, and TTS.

Before the first real voice run, provision the local model assets:

```sh
just voice-setup
just voice-preflight
```

Then start the daemon:

```sh
just voice-daemon
```

For a no-microphone test, use the synthetic capture path instead:

```sh
just voice-demo
```

Without either `voice-daemon` or `voice-demo`, PTT may send a backend request,
but there will be no microphone-to-STT-to-TTS-to-speaker path.

### Terminal 4: Odin gameplay GUI

```sh
just frontend
```

This builds the frontend and opens a borderless full-screen window connected to
`127.0.0.1:7878`. Close the window to stop it. With an Operator cord
connected, hold `PTT / OPERATOR` to test the complete voice path.

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
- `FIRE`
- `TAP BRIDGE 1 LISTEN`
- `TAP BRIDGE 2 LISTEN`

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

```sh
# On the laptop or arcade host. Restrict these ports with the local firewall.
just backend address=0.0.0.0:7878 voice_address=0.0.0.0:7879

# On the Pi, for the Cabinet-side audio transport.
just voice-daemon backend_address=192.168.1.8:7879
```

Replace `192.168.1.8` with the host's LAN address if it changes. Deploy and configure the
Cabinet Frontend from [`python/cabinet_frontend/README.md`](python/cabinet_frontend/README.md),
using `NN_BACKEND_ADDRESS=192.168.1.8:7878`. Start the installed service on
the Pi:

```sh
sudo systemctl start north-neeladesh-hardware-frontend.service
journalctl -u north-neeladesh-hardware-frontend.service -f
```

From the development machine, inspect both services on the Pi at
`taki@192.168.1.34` with `./scripts/status_pi.sh`. The exact service names are
`north-neeladesh-hardware-frontend.service` and
`north-neeladesh-voice-daemon.service`.

The voice daemon is separate from the Cabinet Frontend process. It carries
microphone PCM to the laptop backend and plays returned synthesized audio on
the Cabinet device. The Cabinet Frontend carries controls and hardware output.

### Dummy mode: adapter check only

Dummy mode is not the arcade and is not a second GUI. Its purpose is to let you
develop and test the Cabinet protocol/output boundary on a laptop without GPIO,
SPI, I2C, `/dev/leds0`, or physical hardware. The current dummy input devices
are no-op, so they cannot play the game by themselves.

Stop Odin first, keep the debug backend and debug dashboard running, click
`Reset run`, then run the dummy client as the only game client:

```sh
NN_HARDWARE_MODE=dummy uv run --directory python/cabinet_frontend \
  python -m hardware_frontend --backend-address 127.0.0.1:7878
```

The dummy client prints hardware updates to the terminal. Use the debug
dashboard's `Inject call`, `Advance time`, and `Reset run` controls for output
mapping checks, and check for:

- line lamp updates;
- Directory page updates;
- printer receipts, including a new run after backend restart;
- speaker/audio activity;
- no GPIO, SPI, I2C, or `/dev/leds0` access.

Do not use `Inject call` or debug bypass controls as evidence that the Cabinet
can play the game. They only prove that the Cabinet renders backend snapshots.

Run the Cabinet tests independently with:

```sh
uv run --directory python/cabinet_frontend \
  python -m unittest discover -s hardware_frontend/tests -t . -v
```

## Voice Diagnostics

For a hardware-free relay check, keep the backend running and use another
terminal:

```sh
just voice-smoke
```

For a synthetic capture demo that still exercises the backend workers and
audio return path:

```sh
just voice-demo
```

Voice failure must produce a diagnostic; it must not create a fake Routing or
Story event. For the real microphone/speaker path, use `just voice-daemon` as
described in the GUI test run.

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
- dummy Cabinet output mapping and printer reset reconciliation.

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
