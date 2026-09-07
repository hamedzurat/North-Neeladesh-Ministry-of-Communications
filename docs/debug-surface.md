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

## Hardware Demo

The normal backend command starts the small authored hardware demo. The four-Shift fixture remains available to focused development tests; it is not the live demo configuration.

1. Start `just backend-debug` and `just debug-surface`, then confirm the graph starts at `run_start` with the authored hardware-demo Subscribers and a Directory page for `0001`.
2. Start the Caller and use Directory ID `0002`; confirm the Directory presents Vira Dhal's `RECORDS OFFICE` listing and public note.
3. Connect the Caller to the Operator, attempt a direct Circuit, and confirm the `ring_generator_required` diagnostic appears without a Routing receipt.
4. Connect the requested Callee to the Ring Generator, crank, confirm the Callee Line Lamp lights, then replace the ring connection with a Tap Bridge Circuit.
5. Tune both controls into the clear band, hold and release Police, then hold the matching Tap Bridge listen control. Confirm the dummy audio stream is reported only for the held matching bridge and that Operator Knowledge appears only after listening beyond the first frame.
6. Complete and clear the Circuit. Confirm Police completion, the successful Ending, and all printer receipts, then reset and verify the graph, Calls, counters, printer output, voice evidence, clock, and frontend state return to the authored start.

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
