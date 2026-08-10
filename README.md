# North Neeladesh Ministry of Communications

Offline MVP of the Cabinet Frontend and Rust authority backend.

## Use

```sh
just check      # Rust tests and Odin frontend check
just build      # Release-build both programs
just mvp        # Verify local voice assets and launch the complete MVP
```

`just mvp` is the normal demonstration command. It starts the Rust authority,
uses the prepared offline STT, dialogue, and TTS adapters, and launches the
Cabinet Frontend. See [the local voice setup guide](docs/mvp-local-voice.md)
when the prepared voice workspace is stored somewhere other than its default
location.

For backend/frontend work without local voice services, start the programs in
separate terminals:

```sh
just backend
just frontend
```

The frontend and backend communicate only over loopback TCP (`127.0.0.1:48129`);
no external network access is required.
