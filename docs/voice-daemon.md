# Voice Daemon

The voice daemon is a separate local process. It owns one bounded Operator Session:

```text
PTT start -> microphone capture -> PTT release -> local STT
           -> bounded Response Context -> local dialogue
           -> Qwen3-TTS 1.7B -> RTP/L16 audio and status over UDP
```

The daemon never advances Routing or Story Graph state. The backend remains the sole authority. A worker failure emits `failed` status and a diagnostic; it does not create a Story Event or Routing.

## Worker contracts

`just voice-daemon` uses the checked-in real worker adapters by default. They run from the `python/` uv project and invoke programs and model assets already installed on the target laptop; `--no-sync` prevents the daemon from installing packages or downloading a model at runtime. Run `uv sync --project python` once while provisioning the laptop, then run `just voice-preflight` before the first session.

The smoke workers remain available for protocol-only CI and hardware-free development:

```sh
just voice-smoke
```

The real path requires these local assets and dependencies:

- ALSA `arecord` and `aplay`, with `NN_VOICE_CAPTURE_DEVICE` set to the configured microphone device when `default` is not correct and `playback_device=...` passed to `just voice-daemon` when the playback device is not `default`;
- `whisper.cpp` `whisper-cli` and an English `base.en` model in `NN_WHISPER_MODEL`;
- `llama.cpp` `llama-cli` and the accepted `Qwen3-4B-Instruct-2507` `Q4_K_M` GGUF model in `NN_QWEN3_MODEL`;
- the `python/pyproject.toml` uv environment, containing `torch` and the official `qwen-tts` package, with a local `Qwen3-TTS` 1.7B CustomVoice model directory in `NN_QWEN3_TTS_MODEL`;
- a JSON `NN_QWEN3_VOICE_MAP` mapping Subscriber voice IDs to the installed Qwen3-TTS CustomVoice speakers. The demo's default `taren` ID maps to `Ryan`.

The target laptop profile is Linux with the local microphone exposed through ALSA, roughly 16 GiB of system memory, and an NVIDIA GPU with about 8 GiB of VRAM. Keep Qwen3-4B resident on the GPU when possible; select the TTS device explicitly with `NN_QWEN3_TTS_DEVICE` (default `cuda:0`), or use `cpu` only when the target laptop has been measured to support the latency and memory cost. A Raspberry Pi deployment uses the same worker stdin/stdout and backend UDP contracts with ARM-native local binaries and `NN_QWEN3_TTS_DEVICE=cpu`; its model fit and latency must be measured separately. `NN_LLAMA_EXTRA_ARGS`, `NN_WHISPER_EXTRA_ARGS`, and `NN_VOICE_WORKER_TIMEOUT` allow a pinned local runtime to provide device/thread settings without changing the daemon contract.

Example offline launch configuration:

```sh
export NN_WHISPER_MODEL=/opt/north-neeladesh/models/ggml-base.en.bin
export NN_QWEN3_MODEL=/opt/north-neeladesh/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf
export NN_QWEN3_TTS_MODEL=/opt/north-neeladesh/models/Qwen3-TTS-12Hz-1.7B-CustomVoice
export NN_QWEN3_VOICE_MAP='{"taren":"Ryan"}'
export NN_QWEN3_TTS_DEVICE=cuda:0
just voice-daemon
```

`python/voice_workers/` contains the real adapters. They are launched as `python -m voice_workers.<worker>` inside the uv environment. The real daemon also tees each synthesized PCM packet to local ALSA `aplay`, so the manual path is audible while preserving the existing RTP/L16 UDP path. Their required stdin/stdout contracts are:

- `voice_workers.capture`: starts `arecord` for the configured device and writes bounded signed 16-bit little-endian mono PCM at 16 kHz. PTT release or cancellation terminates it; the 15-second default maximum can be lowered with `NN_VOICE_MAX_UTTERANCE_SECONDS`.
- `voice_workers.stt`: writes the supplied PCM to a temporary WAV, invokes local whisper.cpp `base.en`, and writes one final UTF-8 transcript.
- `voice_workers.dialogue`: sends only the bounded Response Context and transcript to local Qwen3-4B-Instruct-2507 Q4_K_M through `llama-cli`, requests a dialogue-only JSON object, and rejects malformed or overlong output. It cannot emit a Story Event, Routing, or state mutation.
- `voice_workers.tts`: accepts only the Qwen3-TTS 1.7B request contract, maps the Subscriber voice ID to an explicitly configured CustomVoice speaker, and writes signed 16-bit little-endian mono PCM at 24 kHz. It has no fallback engine.

The TTS adapter is intentionally named and validated as Qwen3-TTS 1.7B. Pocket TTS, Piper, and automatic fallback engines are not part of this daemon.

Each provider command has a bounded 30-second deadline. The Rust adapter bounds capture to 15 seconds by default, limits worker output, preserves stderr diagnostics, kills cancelled or timed-out process groups, rejects non-zero exits and malformed output, and emits `failed` or `cancelled` without changing authoritative state. The dialogue worker receives only the active Subscriber Profile, current goal and Call Premise, Story Beat direction, permitted knowledge, beliefs, Relationship Notes, selected Memories, and at most six recent conversation turns. The approximate dynamic context limit is 3,072 tokens.

## Manual test

1. Start the backend: `just backend`.
2. Run `just voice-preflight`, then start the real daemon with `just voice-daemon`.
3. Start Odin with `just frontend` and connect Subscriber 0 to the Operator Jack.
4. Hold the PTT control in Odin. The backend forwards `StartPtt`; the daemon captures microphone audio and reports `listening`.
5. Release PTT. The daemon reports `transcribing`, `generating_response`, `synthesizing`, `playing`, and `completed`, while sending RTP/L16 audio to the backend UDP boundary.
6. Confirm the transcript and response in the daemon output, `speaker_active` in the next backend snapshot, and no Routing receipt was created by voice processing.

For an isolated protocol test, type `ptt` and `release` on its stdin while using `just voice-smoke`. The real path should be manually accepted with a live microphone and the local model assets; it must produce a non-fixed transcript, a Subscriber-specific response, and Qwen3-TTS audio.

The backend UDP port can be changed with `--voice-bind`; pass the matching address as the first positional argument to `just voice-daemon`, for example `just voice-daemon 127.0.0.1:8879`.
