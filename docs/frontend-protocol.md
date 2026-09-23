# Frontend Protocol

The laptop backend is the sole game authority. There is one Cabinet Frontend. It sends one complete `InputMessage` and receives one complete `StateMessage` over a persistent TCP connection.

Each message is framed as:

```text
u32 big-endian payload length | CBOR payload
```

The maximum payload is 4 MiB. The normal frontend protocol version is `3`; the development debug protocol version is `2`. The game cabinet has twelve subscriber lines, one two-jack Tap Bridge, and Police/EMS service controls.

## InputMessage

The wire shape is exactly:

```text
{
  "protocol_version": 3,
  "input_sequence": u64,
  "expected_state_revision": u64,
  "input": {
    "cord_topology": [{"first": PortId, "second": PortId}],
    "held_controls": {
      "ptt": bool, "police": bool, "ems": bool, "tap": bool
    },
    "directory_digits": [u8; 4],
    "ring_line": i16,
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

`PortId` is always one CBOR text string: `subscriber_0` through `subscriber_11`, `operator`, `ring_generator`, or `tap_1` and `tap_2` (the two jacks belonging to one Tap Bridge). A topology has at most eight cords, and every endpoint may occur in at most one cord. Valid physical topologies are accepted even when they do not advance the current call.

`ring_line` is the subscriber line armed by the most recent completed crank rotation, or `-1` when no line is armed. The frontend only arms a line while its Ring Generator cord is connected; the backend validates the resulting circuit.

## StateMessage

The response wire shape is exactly:

```text
{
  "protocol_version": 3,
  "input_sequence": u64,
  "accepted": bool,
  "error": {"code": string, "message": string}|null,
  "state_revision": u64,
  "output": {
    "line_lamps": [bool; 12],
    "game_phase": string,
    "run_generation": u64,
    "clock": {"shift": u8, "elapsed_seconds": u32},
    "speaker_active": bool,
    "interference_level": u8,
    "tap_bridge_audio_active": bool,
    "tuning": {"coarse": u16, "fine": u16},
    "directory_pages": [...],
    "printer_output": [...],
    "call": {...}|null,
    "calls": [{"caller_line": u8, "requested_callee_line": u8, "phase": string}],
    "service_call": {"service": string, "phase": string}|null,
  "tap_bridge_monitoring": {"caller_line": u8, "callee_line": u8, "caller_tap_port": u8, "callee_tap_port": u8}|null,
    "shift": {...},
    "debug": {"messages": [{"code": string, "message": string}]}
  }
}
```

The response never echoes topology, held controls, directory digits, crank timestamps, Cabinet Frontend diagnostics, or Cabinet Frontend/session data. `speaker_active`, `interference_level`, `tap_bridge_audio_active`, `tuning`, `calls`, `service_call`, and `tap_bridge_monitoring` are backend-owned output state. `call` is the Call currently connected to the Operator, while `calls` contains the current Call records, including competing and Held Callers. Terminal Call phases remain visible until their physical topology is cleared. `speaker_active` reflects an active Operator, service, or held Tap Bridge monitoring control. `tap_bridge_audio_active` is true only while the held listen control matches a live Tap Bridge Circuit; it represents the authored dummy monitoring stream. `interference_level` is a bounded percentage for authored Diegetic Interference, where zero is clear. The demo clock maps eight real minutes to the Shift display from `08:00` through `16:00`; consumers interpret `elapsed_seconds` as seconds since midnight, not an elapsed MM:SS duration. Backend diagnostics are only in `output.debug`; Cabinet Frontend diagnostics are only in `input.debug`.

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
just backend-debug
just frontend
```

`backend-debug` is the only live backend entry point. It exposes the authoritative debug command boundary. Voice failures and retained conversation evidence are available through the debug surface rather than backend console tracing. The frontend trace command is:

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

The frontend voice relay uses the backend's UDP voice socket, normally `127.0.0.1:7879`. Status datagrams begin with tag `0x01` and contain a CBOR `VoiceStatusMessage`. PTT control datagrams begin with tag `0x02` and contain a CBOR `VoiceControlMessage`. Captured relay audio begins with tag `0x03` and contains a bounded CBOR `VoiceInputAudioMessage`; the `complete` flag terminates a sequence of 20 ms PCM chunks. Synthesized audio is RTP version 2 with the `L16/24000/1` payload type `96`; samples are signed 16-bit network-order PCM. The Odin frontend and Python Cabinet Frontend both announce `ready`, capture, forward, play, and report recovery status. The laptop backend runs STT, dialogue/LLM, and PocketTTS; the client relay only captures, forwards, plays, and reports recovery status.

Voice status and audio do not advance the authoritative Routing state. Each backend PTT control includes the selected Subscriber voice ID so the daemon does not choose a voice independently. A failed worker produces a backend diagnostic and no Story Event or Routing.

Manual verification:

1. Run `just backend-debug`, `just debug-surface`, and `just frontend` in three terminals.
2. Complete the three-Shift exchange through the Cabinet controls and voice path.
3. Verify that Directory lookup, Ring Generator cranking, Held Callers, Tap Bridge listen control, Police completion, printer receipts, and backend-owned Shift transitions appear on the Cabinet.
4. Repeat the run with a missed Call or omitted Police Service Call. Verify the typed Service Error and that an authored Ending is still reached.
5. Change the Directory Terminal digits and verify the e-paper pages update for the neutral exchange; use an unlisted ID to confirm the no-record display.

For one real voice session, run `uv sync --project python` once during provisioning, configure the offline model assets, run `just voice-preflight`, then run `just backend-debug` and `just frontend`. PTT is controlled by the Odin Frontend; its embedded relay stays listening while idle and carries only audio/status traffic. The Python Cabinet Frontend provides the equivalent relay for Raspberry Pi deployments.
