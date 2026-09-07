# Frontend Protocol

The laptop backend is the sole game authority. There is one Cabinet Frontend. It sends one complete `InputMessage` and receives one complete `StateMessage` over a persistent TCP connection.

Each message is framed as:

```text
u32 big-endian payload length | CBOR payload
```

The maximum payload is 4 MiB. The normal frontend protocol version is `1`; the development debug protocol version is `2`. The larger bound allows the development debug surface to retrieve retained voice audio without changing the normal frontend message shapes.

## InputMessage

The wire shape is exactly:

```text
{
  "protocol_version": 1,
  "input_sequence": u64,
  "expected_state_revision": u64,
  "input": {
    "cord_topology": [{"first": PortId, "second": PortId}],
    "held_controls": {
      "ptt": bool, "police": bool, "ems": bool, "fire": bool,
      "tap_1": bool, "tap_2": bool
    },
    "directory_digits": [u8; 4],
    "crank_rotation_timestamps": [u64; 4],
    "tuning": {"coarse": u16, "fine": u16},
    "debug": {
      "firmware_version": string|null,
      "transport_connected": bool,
      "device_faults": [string]
    }
  }
}
```

`input_sequence` is the request identity. It must be positive and increase for accepted requests. An exact retry of the previous request returns the previous response idempotently. The backend also requires `expected_state_revision` to equal its current revision; rejected requests do not advance that revision.

`PortId` is always one CBOR text string: `subscriber_0` through `subscriber_15`, `operator`, `ring_generator`, or `tap_1` through `tap_4` (the four jacks belonging to two Tap Bridges). A topology has at most eight cords, and every endpoint may occur in at most one cord. Valid physical topologies are accepted even when they do not advance the current call.

The crank array contains the timestamps, in milliseconds, of the last four completed full local rotations. Odin records a timestamp when a rotation completes. It does not send a rotation count or computed speed; the backend validates the chronological history and decides whether a new timestamp satisfies ringing.

## StateMessage

The response wire shape is exactly:

```text
{
  "protocol_version": 1,
  "input_sequence": u64,
  "accepted": bool,
  "error": {"code": string, "message": string}|null,
  "state_revision": u64,
  "output": {
    "line_lamps": [bool; 16],
    "game_phase": string,
    "clock": {"shift": u8, "elapsed_seconds": u32},
    "speaker_active": bool,
    "tuning": {"coarse": u16, "fine": u16},
    "directory_pages": [...],
    "printer_output": [...],
    "call": {...}|null,
    "calls": [{"caller_line": u8, "requested_callee_line": u8, "phase": string}],
    "service_call": {"service": string, "phase": string}|null,
    "tap_bridge_monitoring": u8|null,
    "shift": {...},
    "debug": {"messages": [{"code": string, "message": string}]}
  }
}
```

The response never echoes topology, held controls, directory digits, crank timestamps, Cabinet Frontend diagnostics, or Cabinet Frontend/session data. `speaker_active`, `tuning`, `calls`, `service_call`, and `tap_bridge_monitoring` are backend-owned output state. `call` is the Call currently connected to the Operator, while `calls` contains the current Call records, including competing and Held Callers. Terminal Call phases remain visible until their physical topology is cleared. `speaker_active` reflects an active Operator, service, or held Tap Bridge monitoring control. Backend diagnostics are only in `output.debug`; Cabinet Frontend diagnostics are only in `input.debug`.

## Commands

Run the frontend checks and build:

```sh
just frontend-check
just frontend-build
just build
```

`frontend-build` extracts the regular face from the installed Iosevka TrueType Collection into `target/Iosevka-Regular.ttf` because raylib's loader requires a single-face TTF/OTF file.

Start the backend and frontend separately:

```sh
just backend
just frontend
```

Backend variants are `backend-stress` and `backend-debug`. The debug variant enables printer stress and exposes the authoritative debug command boundary. Voice failures and retained conversation evidence are available through the debug surface rather than backend console tracing. The frontend trace command is:

```sh
just frontend-trace
```

It sets `NN_FRONTEND_TRACE=1`. The default backend address is `127.0.0.1:7878`; use `address=127.0.0.1:7979` for backend commands and `backend_address=127.0.0.1:7979` for frontend commands.

Run the offline TCP/CBOR harness and backend contract tests:

```sh
just protocol-harness
just backend-test
```

The harness verifies string ports, timestamped ringing, routing, arbitrary physical topology acceptance, and invalid input rejection without hardware or an external service.

The Odin frontend probes the backend before creating a window. If the backend later disconnects, the window stays open with an offline status and retries the connection.

## Voice boundary

The separate voice relay uses the backend's UDP voice socket, normally `127.0.0.1:7879`. Status datagrams begin with tag `0x01` and contain a CBOR `VoiceStatusMessage`. PTT control datagrams begin with tag `0x02` and contain a CBOR `VoiceControlMessage`. Captured relay audio begins with tag `0x03` and contains a bounded CBOR `VoiceInputAudioMessage`; the `complete` flag terminates a sequence of 20 ms PCM chunks. Synthesized audio is RTP version 2 with the `L16/24000/1` payload type `96`; samples are signed 16-bit network-order PCM. The relay announces `ready`, then the backend forwards accepted PTT start and release edges to the relay. The laptop backend runs STT, dialogue/LLM, and Qwen3-TTS; the relay only captures, forwards, plays, and reports recovery status.

Voice status and audio do not advance the authoritative Routing state. Each backend PTT control includes the selected Subscriber voice ID so the daemon does not choose a voice independently. A failed worker produces a backend diagnostic and no Story Event or Routing.

Manual verification:

1. Run `just backend` in one terminal and `just frontend` in another.
2. Complete the four authored Shifts: Taren/Vira relief dispatch, Leya/Oren parent-collapse response with tuned interference and EMS, Neri/Vira intercepted Signal with competing Calls and Tap Bridge monitoring, and the final Taren or Oren Routing.
3. Verify that Directory lookup, Ring Generator cranking, Held Callers, Tap Bridge listen control, EMS completion, printer receipts, and backend-owned Shift transitions appear on the Cabinet.
4. Repeat the run with a missed Call or omitted EMS Service Call. Verify the typed Service Error and that an authored Ending is still reached.
5. Change the Directory Terminal digits and verify the e-paper pages update from the backend; use `0002` or `0004` for the final Routing and an unlisted ID for the standoff branch.

For one real voice session, run `uv sync --project python` once during provisioning, configure the offline model assets, run `just voice-preflight`, then run `just backend` and `just voice-daemon`. PTT is controlled by the Cabinet Frontend; the relay stays listening while idle and carries only audio/status traffic. Use `just voice-smoke` for a hardware-free relay test; see `docs/voice-daemon.md` for the model and device configuration.
