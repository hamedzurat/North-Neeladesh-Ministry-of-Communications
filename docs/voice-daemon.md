# Voice Daemon

The voice daemon is a separate Cabinet-side transport and recovery process. The laptop backend owns the bounded Operator Session:

```text
PTT start -> daemon microphone capture -> PTT release -> PCM over UDP
           -> laptop STT -> bounded Response Context -> laptop dialogue
           -> laptop Qwen3-TTS 1.7B -> RTP/L16 audio over UDP -> daemon speaker
```

The daemon never advances Routing or Story Graph state. The backend remains the sole authority. A worker failure emits `failed` status and a diagnostic; it does not create a Story Event or Routing.

## Worker contracts

`just voice-daemon` runs relay-only mode. It does not load STT, dialogue, or Qwen3-TTS and it does not decide a Routing or Story Graph transition. `just backend` configures those workers on the laptop. The relay reports capture/playback failures as typed voice status messages and stays alive so the backend can recover or retry the session.

The backend uses the checked-in real worker adapters. Audio capture and playback on the relay use the Rust `cpal` audio library; the backend STT and dialogue invoke the pacman-installed whisper.cpp and llama.cpp runtimes. The Python workers run from the `python/` uv project; `--no-sync` prevents the backend from installing packages or downloading a model at runtime. Install the runtimes with `sudo pacman -S llama-cpp ggml-cuda whisper-cpp`, provision the model assets with `just voice-setup`, then run `just voice-preflight` before the first session.

The smoke workers remain available for protocol-only CI and hardware-free development:

```sh
just voice-smoke
```

The real path requires these local assets and dependencies:

- a default audio input and output device exposed by the laptop audio stack;
- the pacman-installed whisper.cpp `whisper-cli` and llama.cpp `llama-cli` runtimes;
- the `python/pyproject.toml` uv environment, containing `torch`, `huggingface-hub`, and the official `qwen-tts` package;
- the model assets downloaded by `just voice-setup` into `~/.local/share/north-neeladesh/models`.

The target laptop profile is Linux with a local system audio device, roughly 16 GiB of system memory, and an NVIDIA GPU with about 8 GiB of VRAM. The dialogue worker defaults to a 4,096-token llama.cpp context, GPU offload, and no warmup; override these with `NN_LLAMA_EXTRA_ARGS` only when needed. Keep Qwen3-4B and Qwen3-TTS resident on the GPU when possible; the TTS worker is persistent for real daemon runs and uses PyTorch SDPA, which dispatches to the native CUDA flash-attention kernel when supported. The third-party `flash-attn` package is an optional extra because its released wheels do not currently match this Torch 2.14/CUDA 13 environment. Select the TTS device explicitly with `NN_QWEN3_TTS_DEVICE` (default `cuda:0`). The current laptop run is a local debug profile. The target Raspberry Pi deployment will run only the cpal audio edge: microphone capture and speaker playback stay on the Pi, while the backend on this laptop coordinates STT, dialogue, and Qwen3-TTS over the network. It will not require the Qwen model or Python ML environment on the Pi. `NN_WHISPER_EXTRA_ARGS` and `NN_VOICE_WORKER_TIMEOUT` allow a pinned local runtime to provide device/thread settings without changing the daemon contract.

Model setup and offline launch:

```sh
just voice-setup
just voice-preflight
just voice-daemon
```

The paths above are automatic defaults. `NN_VOICE_MODEL_ROOT`, `NN_WHISPER_MODEL`,
`NN_QWEN3_MODEL`, and `NN_QWEN3_TTS_MODEL` remain optional overrides for a different
installation. Each `SubscriberProfile.voice_id` is the Qwen3-TTS CustomVoice speaker
name directly, such as `Ryan` or `Vivian`; no voice mapping environment variable is
used. Until authored Subscriber profiles are wired into the backend, development runs
select a supported speaker randomly for each request so the voice catalogue can be
tested. `just backend` sets `NN_VOICE_RANDOM_SPEAKER=1`; set it to `0` to pin `Ryan`.

`python/voice_workers/` contains the real model adapters. They are launched by the laptop backend as `python -m voice_workers.<worker>` inside the uv environment. The backend includes the selected Qwen3-TTS CustomVoice `voice_id` in each PTT control message. The relay sends captured PCM to the backend and plays the backend's RTP/L16 output locally. Their required stdin/stdout contracts are:

- the default Rust capture path records from the system audio input, converts it to bounded signed 16-bit mono PCM at 16 kHz, and resamples when the device uses another native rate;
- `voice_workers.stt`: writes the supplied PCM to a temporary WAV, invokes local whisper.cpp `base.en`, and writes one final UTF-8 transcript.
- `voice_workers.dialogue`: sends only the bounded Response Context and transcript to local Qwen3-4B-Instruct-2507 Q4_K_M through `llama-cli`, requests a dialogue-only JSON object, and rejects malformed or overlong output. It cannot emit a Story Event, Routing, or state mutation.
- `voice_workers.tts`: accepts only the Qwen3-TTS 1.7B request contract, uses the Subscriber profile's Qwen3-TTS CustomVoice speaker, and writes framed signed 16-bit little-endian mono PCM at 24 kHz to the persistent daemon worker. It synthesizes sentence-sized chunks and flushes each completed chunk immediately. Qwen's current API simulates incremental text input but does not expose true decoder-frame streaming.

The TTS adapter is intentionally named and validated as Qwen3-TTS 1.7B. Pocket TTS, Piper, and automatic fallback engines are not part of this daemon.

Each provider command has a bounded 30-second deadline. The Rust adapter bounds capture to 15 seconds by default, limits worker output, preserves stderr diagnostics, kills cancelled or timed-out process groups, rejects non-zero exits and malformed output, and emits `failed` or `cancelled` without changing authoritative state. The dialogue worker receives only the active Subscriber Profile, current goal and Call Premise, Story Beat direction, permitted knowledge, beliefs, Relationship Notes, selected Memories, and at most six recent conversation turns. The approximate dynamic context limit is 3,072 tokens.

## Manual test

1. Run `just voice-preflight`, then start the backend with `just backend`.
2. Start the relay with `just voice-daemon` on the laptop or Raspberry Pi.
3. Start Odin with `just frontend` and connect Subscriber 0 to the Operator Jack.
4. Hold the PTT control in Odin. The backend forwards `StartPtt`; the daemon captures microphone audio and reports `listening`.
5. Release PTT. The backend reports `transcribing`, `generating_response`, `synthesizing`, `playing`, and `completed`; the relay sends captured PCM to the backend and plays returned RTP/L16 audio.
6. Confirm the transcript and response in the development debug surface, `speaker_active` in the next backend snapshot, and no Routing receipt was created by voice processing. The relay log should only show connection, recovery, and audio failure summaries.

With no microphone connected, use `just voice-demo` instead of `just voice-daemon`. It uses the synthetic capture as a relay input; the backend still runs the real local STT, Qwen3 dialogue, and Qwen3-TTS workers and the relay plays returned audio. This verifies frontend PTT, backend control forwarding, worker recovery, and audio output without putting model ownership on the relay.

For an isolated protocol test, run `just voice-smoke` against a running backend. Once a microphone is connected, switch back to `just voice-daemon`; the capture path will use the system default input device.

The backend UDP port can be changed with `--voice-bind`; pass the matching address as the first positional argument to `just voice-daemon`, for example `just voice-daemon 127.0.0.1:8879`.
