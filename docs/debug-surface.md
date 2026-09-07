# Development Debug Surface

The debug surface is a development-only web UI. It is separate from the Cabinet Frontend and is bound to loopback by default.

## Start

Run the authoritative backend:

```sh
just backend-debug
```

Run the UI in another terminal:

```sh
just debug-surface
```

Open `http://127.0.0.1:7881`. The backend debug command boundary is `127.0.0.1:7880`; it is not the normal Cabinet TCP protocol on port `7878`.

## Manual Checkpoints

1. Confirm the dashboard shows Run revision, Shift phase, Calls, Subscriber State, Story Graph frontier, current Story Beat, counters, voice status, Cabinet Frontend status, transitions, and recent diagnostics.
2. Click `Advance Time` and confirm the authoritative elapsed time increases.
3. Click `Inject Call` and confirm the Call appears in the dashboard and the transition log.
4. Enable `Bypass Restrictions`, inject a second Call, and confirm both Calls are visible.
5. Select the current Story Graph path or force an authored Story Event and confirm the frontier changes.
6. Enable and disable `Godmode`, then click `Reset Run`; confirm Calls, elapsed time, flags, and Story Graph state return to the authored start.

## Component Checks

| Command | Manual checkpoint |
| --- | --- |
| `just backend` | Connect the Cabinet Frontend and complete one Routing. |
| `just odin` | Confirm physical controls change input and backend-owned output changes on screen. |
| `just debug-surface` | Complete the checkpoints above. |
| `just protocol-harness` | Confirm the offline TCP/CBOR Routing sequence passes. |
| `just content-validation` | Confirm the authored Story Graph tests pass. |
| `just backend-tests` | Confirm backend contract and runtime tests pass. |
| `just frontend-checks` | Confirm Odin type checking passes. |
| `just voice-checks` | Confirm the voice boundary and recovery tests pass. |

The UI intentionally has no gameplay authority. Every control is converted to a typed `DebugCommand` and applied by the Rust backend.

## Voice Deployment Boundary

The laptop backend is the authority and the worker host for STT, dialogue/LLM, and Qwen3-TTS. The voice daemon is a Cabinet-side transport and recovery process: it captures or plays audio on the remote device, forwards typed status/audio datagrams, retries the backend association, and does not decide Story Graph or Routing outcomes. Keep the UDP voice address reachable between the Raspberry Pi and laptop; do not expose the debug TCP or HTTP ports outside the trusted development network.
