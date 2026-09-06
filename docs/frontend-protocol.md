# Frontend Protocol

The laptop backend is the sole game authority. There is one Cabinet Frontend. It sends one complete `InputMessage` and receives one complete `StateMessage` over a persistent TCP connection.

Each message is framed as:

```text
u32 big-endian payload length | CBOR payload
```

The maximum payload is 1 MiB. The protocol version is `1`.

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
    "shift": {...},
    "debug": {"messages": [{"code": string, "message": string}]}
  }
}
```

The response never echoes topology, held controls, directory digits, crank timestamps, Cabinet Frontend diagnostics, or Cabinet Frontend/session data. `speaker_active` and `tuning` are backend-owned output state. `speaker_active` reflects an active Operator, service, or Tap Bridge audio control. Backend diagnostics are only in `output.debug`; Cabinet Frontend diagnostics are only in `input.debug`.

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

Backend variants are `backend-trace`, `backend-stress`, and `backend-debug`. The trace variant sets `NN_BACKEND_TRACE=1`; the debug variant also enables printer stress. The frontend trace command is:

```sh
just frontend-trace
```

It sets `NN_FRONTEND_TRACE=1`. Both traces print the new CBOR shapes. The default backend address is `127.0.0.1:7878`; use `address=127.0.0.1:7979` for backend commands and `backend_address=127.0.0.1:7979` for frontend commands.

Run the offline TCP/CBOR harness and backend contract tests:

```sh
just protocol-harness
just backend-test
```

The harness verifies string ports, timestamped ringing, routing, arbitrary physical topology acceptance, and invalid input rejection without hardware or an external service.

The Odin frontend probes the backend before creating a window. If the backend later disconnects, the window stays open with an offline status and retries the connection.

## Voice boundary

The separate voice daemon uses the backend's UDP voice socket, normally `127.0.0.1:7879`. Status datagrams begin with tag `0x01` and contain a CBOR `VoiceStatusMessage`. PTT control datagrams begin with tag `0x02` and contain a CBOR `VoiceControlMessage`. Synthesized audio is RTP version 2 with the `L16/24000/1` payload type `96`; samples are signed 16-bit network-order PCM. The daemon announces `ready`, then the backend forwards accepted PTT start and release edges to the daemon.

Voice status and audio do not advance the authoritative Routing state. Each backend PTT control includes the selected Subscriber voice ID so the daemon does not choose a voice independently. A failed worker produces a backend diagnostic and no Story Event or Routing.

Manual verification:

1. Run `just backend` in one terminal.
2. Run `just frontend` in another terminal.
3. Leave the initial directory selection at `0001`, connect Subscriber 0 to the Operator Jack, and verify the call enters the Operator Session.
4. Connect Subscriber 1 to the Ring Generator, crank until the backend reports Ringing, then connect Subscriber 0 directly to Subscriber 1 and verify the printer records the Routing.
5. Change the Directory Terminal digits and verify the e-paper pages update from the backend.

For one real local voice session, run `uv sync --project python` once during provisioning, configure the offline model assets, run `just voice-preflight`, then run `just voice-daemon` after starting `just backend`. The daemon accepts `ptt` and `release` on stdin as a manual fallback for testing the same session boundary used by backend PTT controls. Use `just voice-smoke` only for a hardware-free protocol test; see `docs/voice-daemon.md` for the model and device configuration.
