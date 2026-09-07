# Current Status: Hardware Demo

Date: 2026-09-07

## Purpose

The immediate target is a hardware-first demo, not the full Wayfinder game.
The durable work is the backend authority, protocol, Odin simulator, voice
transport, Cabinet frontend, and dummy hardware boundary. The story content is
temporary and replaceable.

Deferred until after the hardware loop is truthful:

- Seed persistence, Saves, Recovery Points, and Replay.
- The full Story Graph and final story writing.
- Large Subscriber and Faction catalogues.
- Subscriber Memories, Actions, and State Queries.
- Household economy and Ministry progression.
- More than two Tap Bridges.
- Final manufacturing and physical wiring.

## Target Demo

One repeatable accelerated Shift:

- Eight real minutes represent `08:00` through `16:00`.
- Backend owns the displayed clock.
- Odin and the Pi frontend render the same snapshots.
- A temporary authored story exercises normal Routing, interference/tuning,
  competing and Held Callers, both Tap Bridges, Directory use, all three
  Service controls, printer output, voice activity, and reset.
- Missing physical parts use Odin or dummy components with the same protocol
  semantics.

The player-facing mechanical sequence is:

1. Caller lamp and Waiting Call.
2. Caller to Operator.
3. Directory lookup using a numeric Subscriber ID.
4. Operator conversation through PTT.
5. Requested Callee to Ring Generator.
6. Crank until the Callee lamp rings.
7. Ring connection removed.
8. Direct or Tap Bridge Circuit created.
9. Connected Call cleared.
10. Service controls, interference, and Tap monitoring exercised at authored
    moments.
11. Shift settles, receipts are visible, and reset permits a clean repeat.

## Repository Snapshot

Current `HEAD` is `a4859af` on `master`. The audited commits are:

- `a4859af feat: make hardware demo locally runnable`
- `bc164a1 feat: make hardware demo repeatable`
- `1505fb2 fix: close hardware demo review gaps`
- `912cd27 fix: align cabinet demo outputs`
- `6065c15 fix: enforce ringing before routing`

This implementation pass changes backend routing/service behavior, the protocol
harness, the Cabinet printer mapper, and their tests. The protected frontend
architecture and hardware deployment scripts were not changed beyond the
printer reset reconciliation needed for repeatability.

## Verified Passing

The following checks pass for this implementation:

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `just protocol-harness`
- `just frontend-check`
- `just voice-checks`
- `just voice-preflight`
- `uv run --directory python/cabinet_frontend python -m unittest discover -s hardware_frontend/tests -t . -v`

The Cabinet frontend test suite now reports 20 passing tests. The backend
Cabinet mechanics suite now reports 12 passing tests.

These are contract, dummy, and local development checks. They are not physical
Raspberry Pi or real microphone acceptance. The root [`README.md`](../README.md)
contains the repeatable local runbook using `backend-debug`, the debug surface,
Odin, and the dummy Cabinet Frontend.

## Implemented

### Backend and protocol

- Accelerated clock maps eight real minutes to an eight-hour Shift.
- Debug time advance uses the same clock conversion.
- Reset returns the backend clock to `08:00`.
- Backend owns Calls, Routing, line lamps, Directory pages, printer output,
  Shift counters, Service state, Tap topology, interference state, voice
  state, and Story Graph state.
- TCP length-framed CBOR carries complete input and output snapshots.
- Odin sends Cord topology, held controls, Directory digits, crank timestamps,
  tuning, and debug diagnostics.
- Direct connections before Ring Generator/crank preserve the Call instead of
  creating a Routing receipt.
- Direct and Tap connections before Ring Generator/crank return
  `accepted: false` with `ring_generator_required`, preserve the Call, do not
  advance `state_revision`, and create no Routing receipt.
- Routing attempts with a Directory selection that does not match the authored
  Call return `accepted: false` with `directory_selection_required`.
- Police, EMS, and Fire are represented by the shared `ServiceKind` and generic
  press/release tests pass.
- A required ServiceKind must match the held Service control; the wrong type
  does not increment completion.
- The hardware demo assigns Police, EMS, and Fire requirements to its three
  authored mechanical Shifts.
- Two Tap Bridge controls are represented and only the matching held control
  reports monitoring.
- Interference state and tuning are visible in backend snapshots and Odin.
- Voice selection is derived from authored Subscriber identity rather than a
  random speaker choice.

### Cabinet frontend

- Dummy components exist for lamps, seven-segment clock, Directory display,
  printer, audio activity, rotary input, and Cord scanning.
- Real component adapters exist for the currently wired lamps, TM1637,
  e-paper, rotary encoder, and pair detector.
- Keyboard controls remain available for local development.
- The frontend maps backend snapshots without game-specific story logic.

Odin and the Cabinet Frontend are interchangeable protocol clients, but only
one gameplay input stream should be connected to the backend at a time. Run
the dummy Cabinet after stopping Odin when checking Cabinet output mapping;
otherwise the clients compete over input sequence and state revision.

## Findings Blocking the Target

### Completed P0: Directory selection is now authoritative

`crates/backend/src/lib.rs:675-679` records the Directory digits, but Routing
in `crates/backend/src/lib.rs:1697-1779` uses the Call's stored Callee. After a
Call is created for `0002`, changing the Directory to `0001` does not prevent a
direct or Ring Generator route to the original Callee.

Implemented behavior:

- The current authored Call determines the valid numeric Directory selection.
- A mismatched or unknown selection blocks a Routing attempt.
- The backend emits a diagnostic and no Routing receipt.
- Regression coverage includes a valid Call followed by a wrong Directory
  choice. The implementation reads authored Directory IDs from the Call
  Premise rather than relying on a fixed probe range.

### Completed P0: Pre-ring input is rejected at the protocol level

The state guard and final response now agree. Required behavior is implemented:

- `accepted` is `false` for direct or Tap connection before ringing.
- The error code is `ring_generator_required`.
- The Call remains unchanged.
- No Routing receipt or completed Routing counter is created.

### Completed P0: Required Service validation matches ServiceKind

`apply_service_transition` now validates the required type for every ServiceKind:

- A required Police, EMS, or Fire call accepts only that type.
- A wrong type emits a typed diagnostic and does not increment completion.
- The hardware story visibly exercises Police, EMS, and Fire.

### Completed P0: Reset printer reconciliation

The backend restarts printer IDs at `1` in `crates/backend/src/lib.rs:2090-2098`.
`OutputMapper` retains `_seen_printer_entries` across snapshots at
`python/cabinet_frontend/hardware_frontend/output_mapper.py:33,70-76`.
After reset, reused IDs can be treated as already printed.

Implemented behavior:

- A new run has a recognizable printer epoch or decreasing ID sequence.
- The Cabinet clears its seen-entry set for that new run.
- A second run prints its reset, Routing, Service, and Ending receipts.

### Partially completed P1: The hardware Story Graph now has three Calls

`AuthoredContent::hardware_demo()` now contains three Call nodes:

1. Taren to Vira for normal numeric Directory Routing.
2. Neri to Vira with authored interference and tuning.
3. Leya to Oren for a Tap Bridge route.

The graph still does not authoritatively exercise:

- A competing Caller and Held Caller.
- Both Tap Bridges in the authored flow.
- A complete end-to-end three-Shift runtime test.

The current test proves graph compilation and node shape. It does not yet prove
the complete player sequence.

### P1: Tap Bridge audio remains a state flag

The backend sets `tap_bridge_audio_active` at
`crates/backend/src/lib.rs:802-809`. The dummy audio component receives only
`set_active` and interference levels. No NPC-to-NPC exchange is routed through
the Tap Bridge, and the knowledge fact is granted after two snapshots rather
than after an authored fact is actually heard.

Required demo behavior:

- A bounded deterministic NPC exchange exists for the Tap Call.
- It is audible only while the matching Tap control is held.
- Knowledge becomes available only after the authored fact is heard.
- Releasing the control immediately stops monitoring and audio.
- Tap monitoring itself never advances the Story Graph.

### P1: Real Pi controls are not wired

`python/cabinet_frontend/hardware_frontend/components/factory.py:67-90`
creates `NoopControls` unless keyboard mode is enabled. There is no real GPIO
mapping yet for PTT, Police, EMS, Fire, Tap 1, Tap 2, or Directory digits.
Tuning also has no physical input and defaults to `0/0`.

The current real component path can exercise some output and topology devices,
but the complete physical Cabinet loop is not yet accepted. Until controls are
wired, Odin, keyboard mode, and dummy components remain valid development
clients.

### P1: Voice context is only partly Subscriber-authored

The backend voice ID selection uses the authored Subscriber mapping, but
`demo_response_context` still selects profile details by line number. Moving a
Subscriber to another line can mismatch the voice and context. The stale
random-speaker wording also remains in `docs/voice-daemon.md`.

### P2: Physical acceptance is not recorded

No completed manual run is recorded for:

- Real microphone to STT.
- Local dialogue generation.
- Qwen3-TTS to speaker.
- Physical Pi controls.
- Physical printer and audio.
- Reset and repeat on the actual Cabinet.

## Implementation Queue

Work from the top down. Do not start deferred game systems.

### Completed Step 1: Regression tests and contract fixes

- Added a wrong-Directory Routing test.
- Changed pre-ring responses to `accepted: false` with typed errors.
- Added required ServiceKind tests and a runtime Police/EMS/Fire Shift test.
- Added an OutputMapper reset/receipt test.
- Added Tap pre-ring rejection coverage.

### Completed Step 2: Make current mechanics authoritative

- Every route attempt is gated by the selected Directory record.
- Required ServiceKind is matched, not just completion count.
- Printer IDs are reconciled using the backend-owned `run_generation` in each
  snapshot.
- Pre-ring rejection preserves the existing Call and diagnostic while keeping
  the state revision unchanged.

### Partially completed Step 3: Expand the temporary hardware story

Author a small linear sequence of Shift Calls:

1. Taren requests Vira using Directory `0002` and completes a normal route.
2. Neri's Call has interference and requires tuning.
3. A Tap Bridge Call exposes a bounded intercepted exchange and a competing
   Held Caller.
4. Police, EMS, and Fire each produce visible Service behavior at deterministic
   points.
5. The Shift settles, prints an Ending, and resets cleanly.

The graph and runtime service progression are deterministic. Competing/Held
Caller behavior, both authored Tap Bridges, and a complete end-to-end story
sequence still need to be added to this specific hardware fixture. Do not add
persistence or final story prose.

### Step 4: Add deterministic dummy Tap audio

- Use a bounded authored exchange rather than a new general dialogue system.
- Expose a stream/event through the existing audio boundary.
- Make the dummy component record whether audio was active while held.
- Grant the fact from the authored exchange completion.

### Step 5: Complete the local physical boundary

- Add GPIO-backed controls when the hardware pin assignment is available.
- Add a dummy tuning source with the same `0..1023` contract.
- Keep the frontend generic: it translates input and renders backend output.
- Do not move story rules into Python.

### Step 6: Manual acceptance

- Run the exact authored sequence in Odin.
- Run the same sequence against the dummy Cabinet frontend.
- Reset and repeat the sequence twice.
- Record one successful and one failed real voice run.
- Run physical smoke tests only for connected components.

## Definition Of Done

- One eight-minute dummy Shift resets and repeats.
- Directory selection controls the requested Callee.
- Direct connection before ringing is protocol-rejected.
- Ringing requires the Ring Generator and crank.
- Both Tap Bridges can be exercised and release stops monitoring.
- Competing and Held Callers work in the authored sequence.
- Police, EMS, and Fire each produce correct Service state and receipts.
- Wrong ServiceKind cannot satisfy a required Service rule.
- Tuning reduces authored interference and permits the affected route.
- Clock values match in Odin and the Pi frontend.
- Printer receipts appear again after reset.
- Directory pages show numeric IDs and useful public records without private
  facts.
- Voice identity is stable per authored Subscriber.
- The dummy Cabinet uses only the shared backend contract.
- Physical acceptance results are recorded separately from automated tests.
