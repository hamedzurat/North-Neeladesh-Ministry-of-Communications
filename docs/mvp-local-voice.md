# Local MVP voice configuration

Run the offline MVP in separate terminals:

```sh
# Terminal 1
just backend

# Terminal 2
just frontend
```

`just backend` verifies the prepared local Whisper, Qwen, and Pocket TTS assets before starting the Rust authority. Its terminal then retains the stage logs while the frontend runs separately. Both programs make no network request. The default asset workspace is `$HOME/.local/share/north-neeladesh/voice-benchmark`; set `NN_MVP_VOICE_BENCHMARK_ROOT` to use another prepared local workspace.

## Required local commands

The included adapters use the selected local benchmark workspace. To replace any stage, set these environment variables to executable, already-prepared local adapters:

```sh
export NN_MVP_STT_COMMAND=/absolute/path/to/local-stt
export NN_MVP_DIALOGUE_COMMAND=/absolute/path/to/local-dialogue
export NN_MVP_TTS_COMMAND=/absolute/path/to/local-tts
```

The adapters intentionally have a small, inspectable contract:

- `local-stt <captured-wav>` writes a transcript to stdout.
- `local-dialogue` receives the bounded Subscriber Profile prompt on stdin and writes one reply to stdout. It must use the local Qwen/llama.cpp model.
- `local-tts <voice-configuration> <output-wav>` receives the reply on stdin, writes a WAV file at `output-wav`, and uses the supplied local voice configuration. The included Pocket adapter uses profile-specific pitch and pace settings, so the four hardcoded Profiles remain audibly distinct without adding private voice recordings to Git.

`pw-record` records the microphone at 16 kHz mono while PTT is held. `paplay` plays the generated response unless the existing Cabinet speaker control is disabled. Set `NN_MVP_PLAY_COMMAND` to use another local playback executable.

The Rust core passes only the active Subscriber Profile, paired relationship, immediate goal, and the Operator's transcript to the dialogue adapter. Replies are trimmed to 35 words. An empty first reply is retried once; a second empty reply uses the Subscriber's deterministic routing-request fallback.

If capture, STT, dialogue, TTS, or playback cannot run, the thermal-printer feed and routing status show the failed stage. The Call Attempt remains at the Operator Jack: hold PTT to retry, or press `R` to reset the demonstration. PTT release may wait up to 30 seconds for the bounded local stages so a healthy model turn is never misreported as a disconnected backend.
