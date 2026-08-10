# North Neeladesh Ministry of Communications

Offline MVP of the Cabinet Frontend and Rust authority backend.

## Use

```sh
just check      # Rust tests and Odin frontend check
just build      # Release-build both programs
```

Run the MVP in separate terminals so each component's logs remain visible:

```sh
# Terminal 1
just stt

# Terminal 2
just dialogue

# Terminal 3
just tts

# Terminal 4
just backend

# Terminal 5
just frontend
```

See [the local voice setup guide](docs/mvp-local-voice.md) when the prepared
voice workspace is stored somewhere other than its default location.

The frontend and backend communicate only over loopback TCP (`127.0.0.1:48129`);
no external network access is required.
