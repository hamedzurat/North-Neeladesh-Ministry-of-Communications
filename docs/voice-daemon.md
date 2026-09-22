# Voice Transport

The Odin and Python Cabinet Frontends own the Cabinet-side transport and recovery loop. The laptop backend owns the bounded Operator Session:

```text
PTT start -> frontend microphone capture -> PTT release -> PCM over UDP
           -> laptop STT -> bounded Response Context -> laptop dialogue
             -> laptop Qwen3-TTS or PocketTTS -> RTP/L16 audio over UDP -> frontend speaker
```

The daemon never advances Routing state. The backend remains the sole authority. A worker failure emits `failed` status and a diagnostic; it does not create a routing decision.

## Worker contracts

The Odin relay is compiled into `./frontend`; the Python relay runs as an
embedded `cabinet_frontend.voice_relay` thread inside the Raspberry Pi
frontend. Neither relay loads
STT, dialogue, or PocketTTS and neither decides a Routing transition. The
backend configures those workers and remains authoritative. The relay reports
capture/playback failures as typed voice status messages and stays alive so the
backend can recover or retry the session.

The backend uses the checked-in real worker adapters. The Pi relay uses Python and the system `arecord`/`aplay` tools, so the Pi needs no Rust toolchain or voice binary. The backend STT and dialogue invoke the pacman-installed whisper.cpp and llama.cpp runtimes. The Python workers run from the `python/` uv project; `--no-sync` prevents the backend from installing packages or downloading a model at runtime. Install the runtimes with `sudo pacman -S llama-cpp ggml-cuda whisper-cpp`, provision the model assets with `just voice-setup`, then run `just voice-preflight` before the first session.

The real path requires these local assets and dependencies:

- a default audio input and output device exposed by the laptop audio stack;
- the pacman-installed whisper.cpp `whisper-cli` runtime and a local Ollama service;
- the `python/pyproject.toml` uv environment, containing `torch`, `huggingface-hub`, `qwen-tts`, and `pocket-tts`;
- the model assets downloaded by `just voice-setup` into `~/.local/share/north-neeladesh/models`.

The target laptop profile is Linux with a local system audio device, roughly 16 GiB of system memory, and a GPU for Ollama. The dialogue worker keeps Ollama model `qwen3.5:4b` resident with `keep_alive: -1` and disables reasoning with `think: false`. PocketTTS remains persistent and CPU-only. The current laptop run is a local debug profile. The target Raspberry Pi deployment will run the Python audio edge: microphone capture and speaker playback stay on the Pi, while the backend on this laptop coordinates STT, Ollama dialogue, and PocketTTS over the network. `NN_WHISPER_EXTRA_ARGS` and `NN_VOICE_WORKER_TIMEOUT` allow a pinned local runtime to provide device/thread settings without changing the daemon contract.

Model setup and offline launch:

```sh
just voice-setup
just voice-preflight
just frontend
```

The paths above are automatic defaults. `NN_VOICE_MODEL_ROOT`, `NN_WHISPER_MODEL`,
`NN_QWEN3_MODEL`, and `NN_QWEN3_TTS_MODEL` remain optional overrides for a different
installation. Each `SubscriberProfile.voice_id` is the Qwen3-TTS CustomVoice speaker
name directly, such as `Ryan` or `Vivian`; no voice mapping environment variable is
used. The active neutral Call selects the subscriber voice context for each request.

`python/voice_workers/` contains the real model adapters. They are launched by the laptop backend as `python -m voice_workers.<worker>` inside the uv environment. The backend includes the selected Qwen3-TTS CustomVoice `voice_id` in each PTT control message. The relay sends captured PCM to the backend and plays the backend's RTP/L16 output locally. Their required stdin/stdout contracts are:

- the Odin and Python capture paths record bounded signed 16-bit mono PCM at 16 kHz from the system audio input;
- `voice_workers.stt`: writes the supplied PCM to a temporary WAV, invokes local whisper.cpp `base.en`, and writes one final UTF-8 transcript.
- `voice_workers.dialogue`: sends each bounded Response Context and transcript to local Ollama using `qwen3.5:4b`, `think: false`, JSON format, and `keep_alive: -1`. It requests a dialogue-only JSON object and rejects malformed or overlong output. It cannot emit a Story Event, Routing, or state mutation.
- `voice_workers.tts`: accepts only the Qwen3-TTS 1.7B request contract, uses the Subscriber profile's Qwen3-TTS CustomVoice speaker, and writes framed signed 16-bit little-endian mono PCM at 24 kHz to the persistent daemon worker. It synthesizes sentence-sized chunks and flushes each completed chunk immediately. Qwen's current API simulates incremental text input but does not expose true decoder-frame streaming.

PocketTTS always runs on the CPU and uses twelve official precomputed English voice embeddings in `python/.models/`, one for each subscriber line. The embedding files are ignored by Git and are downloaded by `just voice-setup`.

Each provider command has a bounded 30-second deadline. The Rust adapter bounds capture to 15 seconds by default, limits worker output, preserves stderr diagnostics, kills cancelled or timed-out process groups, rejects non-zero exits and malformed output, and emits `failed` or `cancelled` without changing authoritative state. The dialogue worker receives only the active Subscriber Profile, current goal and Call Premise, Story Beat direction, permitted knowledge, beliefs, Relationship Notes, selected Memories, and at most six recent conversation turns. The approximate dynamic context limit is 3,072 tokens.

## Manual test

1. Run `just voice-preflight`, then start the backend with `just backend-debug`.
2. Start Odin with `just frontend` and connect Subscriber 0 to the Operator Jack.
3. Hold the PTT control in Odin. The backend forwards `StartPtt`; the embedded relay captures microphone audio and reports `listening`.
4. Release PTT. The backend reports `transcribing`, `generating_response`, `synthesizing`, `playing`, and `completed`; the relay sends captured PCM to the backend and plays returned RTP/L16 audio.
6. Confirm the transcript and response in the development debug surface, `speaker_active` in the next backend snapshot, and no Routing receipt was created by voice processing. The relay log should only show connection, recovery, and audio failure summaries.

The backend UDP port can be changed with `--voice-bind`; set the matching
`NN_VOICE_BACKEND_ADDRESS` before `just frontend`. The Rust package currently
remains in the workspace because the backend imports its worker adapters; its
standalone relay binary is no longer part of the Odin run.
