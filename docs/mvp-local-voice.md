# Local MVP voice configuration

Run each offline component in its own terminal, in this order:

```sh
# Terminal 1
just stt

# Terminal 2
just llm

# Terminal 3
just tts

# Terminal 4
just backend

# Terminal 5
just frontend
```

`just stt`, `just llm`, and `just tts` own the local Whisper, Qwen, and Pocket TTS workers respectively. `just backend` is only the Rust authority; it calls those three hardcoded loopback-only workers when an Operator Session finishes. The prepared asset workspace is `$HOME/.local/share/north-neeladesh/voice-benchmark`.

The justfile owns the local ports: STT `18080`, LLM `18081`, TTS `18082`, and backend `48129`. Keep the matching `just` recipes together; there is no command-path or endpoint setup to export.

## Required local commands

The hardcoded adapters intentionally have a small, inspectable contract:

- `local-stt <captured-wav>` writes a transcript to stdout.
- `local-dialogue` receives the bounded Subscriber Profile prompt on stdin and writes one reply to stdout. It must use the local Qwen/llama.cpp model.
- `local-tts <voice-configuration> <output-wav>` receives the reply on stdin, writes a WAV file at `output-wav`, and uses the supplied local voice configuration. The included Pocket adapter uses profile-specific pitch and pace settings, so the four hardcoded Profiles remain audibly distinct without adding private voice recordings to Git.

`pw-record` records the microphone at 16 kHz mono while PTT is held. `paplay` plays the generated response unless the existing Cabinet speaker control is disabled. Set `NN_MVP_PLAY_COMMAND` to use another local playback executable.

The Rust core passes only the active Subscriber Profile, paired relationship, immediate goal, and the Operator's transcript to the dialogue adapter. Replies are trimmed to 35 words. An empty first reply is retried once; a second empty reply uses the Subscriber's deterministic routing-request fallback.

If capture, STT, dialogue, TTS, or playback cannot run, the thermal-printer feed and routing status show the failed stage. The Call Attempt remains at the Operator Jack: hold PTT to retry, or press `R` to reset the demonstration. PTT release may wait up to 30 seconds for the bounded local stages so a healthy model turn is never misreported as a disconnected backend.
