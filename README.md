# North Neeladesh Ministry of Communications

Offline MVP of the Cabinet Frontend and Rust authority backend.

## Use

```sh
just check      # Rust tests and Odin frontend check
just build      # Release-build both programs
```

Run the MVP in two terminals so the Rust authority's local STT, dialogue, and
TTS logs remain visible while you use the Cabinet Frontend:

```sh
# Terminal 1: verifies prepared local voice assets, then starts Rust
just backend

# Terminal 2: starts the Cabinet Frontend
just frontend
```

See [the local voice setup guide](docs/mvp-local-voice.md) when the prepared
voice workspace is stored somewhere other than its default location.

The frontend and backend communicate only over loopback TCP (`127.0.0.1:48129`);
no external network access is required.
