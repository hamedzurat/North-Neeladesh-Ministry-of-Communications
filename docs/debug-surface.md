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

1. Confirm the dashboard shows Run revision, Shift phase, Calls, all five authored Subscriber States, the full authored Story DAG with current/frontier highlighting, Story Graph frontier, current Story Beat, counters, voice status, Cabinet Frontend status, exact frontend input/output JSON, retained voice conversations, and failures.
2. Click `Advance Time` and confirm the authoritative elapsed time increases.
3. Click `Inject Call` and confirm the Call appears in the dashboard and the current authoritative state revision changes.
4. Enable `Bypass Restrictions`, inject a second Call, and confirm both Calls are visible.
5. Select the current Story Graph path or force an authored Story Event and confirm the frontier changes.
6. Enable and disable `Godmode`, then click `Reset Run`; confirm Calls, elapsed time, flags, and Story Graph state return to the authored start.
7. Complete a voice turn and confirm the retained conversation shows the captured input, STT transcript, LLM response, synthesized output, sample counts, and replay controls.
8. Trigger a failed voice turn and confirm the failure banner and retained conversation show the provider error.

## Four-Shift Demo

The normal backend command starts the complete authored demo. The smaller `Backend::new()` fixture remains available to focused one-Shift contract tests; it is not the live demo configuration.

1. Start `just backend-debug` and `just debug-surface`, then confirm the graph starts at `run_start` with five Subscribers: Taren Kesh, Vira Dhal, Dr. Leya Varan, Captain Oren Vey, and Neri Tal.
2. In Shift 1, use Directory IDs `0001` and `0002`, answer Taren, crank the Ring Generator, and route Taren to Vira. Clear the Circuit and confirm the graph enters Shift 2, `relief_train_7` is printed, and the Call list resets.
3. In Shift 2, answer Dr. Leya, tune both controls into the clear band, hold EMS, release it, then route the parent-collapse Call to Oren. Confirm the interference indicator, EMS receipt, and no Service Error.
4. In Shift 3, answer Neri while another Caller is Held, route the Circuit through a Tap Bridge, and hold then release its listen control. Confirm `tap_bridge_monitoring` follows the control, the intercepted fact appears in Operator Knowledge, and the graph reaches `final_choice`.
5. At `final_choice`, use Directory ID `0002` for Taren and Trade Détente, `0004` for Oren and Managed Emergency Rule, or an unlisted ID for the standoff. Complete the final Routing where selected. Exactly one Ending is printed.
6. Repeat the run with the required Call or EMS Call missed. Confirm authored fallback progression, the typed Service Error counter, and a terminal Ending still occur.
7. Reset the Run and confirm the graph, Calls, counters, printer output, voice evidence, elapsed time, and frontend state return to the authored start.

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

The UI intentionally has no gameplay authority. Every control is converted to a typed `DebugCommand` and applied by the Rust backend. The debug command server and UI bind to loopback addresses only. The backend retains voice conversations in memory for the current development run; captured input and synthesized output are fetched on demand as WAV files and are cleared by `Reset Run`.

## Voice Deployment Boundary

The laptop backend is the authority and the worker host for STT, dialogue/LLM, and Qwen3-TTS. The voice daemon is a Cabinet-side transport and recovery process: it captures or plays audio on the remote device, forwards typed status/audio datagrams, retries the backend association, and does not decide Story Graph or Routing outcomes. Keep the UDP voice address reachable between the Raspberry Pi and laptop; do not expose the debug TCP or HTTP ports outside the trusted development network.
