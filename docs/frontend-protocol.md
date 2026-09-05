# Frontend Snapshot Protocol

The laptop backend is the sole authority. Odin and the Cabinet Frontend use the same logical contract: submit one complete `InputMessage`, receive one complete `StateMessage`.

Control messages use a persistent TCP connection. Each message is encoded as:

```text
u32 big-endian payload length | CBOR payload
```

The maximum payload is 1 MiB. CBOR maps use the field names in the Rust protocol types; enums use their snake-case names. The protocol version is `1`.

## Request

`InputMessage` contains `session_id`, a unique `message_id`, the caller's `expected_state_revision`, and a complete `InputSnapshot`:

- frontend identity and input sequence;
- complete Cord Topology;
- all held controls, including four Tap Bridge controls;
- four Directory digits;
- crank rotation/speed and coarse/fine tuning;
- reset request; and
- frontend firmware, transport, and device diagnostics.

## Response

Every accepted or rejected semantic request returns a `StateMessage` with a complete `StateSnapshot`. Rejection does not advance the authoritative state revision or input sequence. The full response still includes the latest state so a frontend can resynchronize without a second request.

The state includes:

- frontend identity, session, sequence, revision, Cord Topology, held controls, Directory digits, crank, and tuning;
- reset result, sixteen Line Lamps, game phase, clock, current Call, and Shift status;
- all current Directory pages;
- append-only printer output; and
- frontend diagnostics plus backend diagnostic messages.

The backend accepts a message only when its protocol version, session, controller identity, state revision, sequence, physical ports, digits, and analog ranges are valid. A duplicate request with the same complete message is returned idempotently.

## Manual checkpoints

Start the backend:

```sh
just backend
```

In another terminal, run the offline TCP/CBOR harness:

```sh
just protocol-harness
```

The harness starts an isolated loopback backend and verifies the authored Caller Line Lamp, Operator Circuit, Ring Generator and crank prerequisites, direct Routing, Circuit clearing, reset, and invalid-input rejection. It repeats a valid Routing after reset. It does not contact an external service or require hardware. `just backend-test` runs the reducer contract tests; `just check` runs workspace typechecking.

To send the same checkpoints to a separately running backend, use:

```sh
just protocol-manual
```
