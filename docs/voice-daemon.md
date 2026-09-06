# Voice Daemon

The voice daemon is a separate local process. It owns one bounded Operator Session:

```text
PTT start -> microphone capture -> PTT release -> local STT
           -> bounded Response Context -> local dialogue
           -> Qwen3-TTS 1.7B -> RTP/L16 audio and status over UDP
```

The daemon never advances Routing or Story Graph state. The backend remains the sole authority. A worker failure emits `failed` status and a diagnostic; it does not create a Story Event or Routing.

## Worker contracts

`just voice-daemon` runs the checked-in smoke workers by default. They validate the complete local process and UDP path without model downloads or audio hardware. To use real workers, override the corresponding Justfile parameters:

```sh
just voice-daemon \
  capture_command="python3 /path/to/capture.py" \
  stt_command="python3 /path/to/stt.py" \
  dialogue_command="python3 /path/to/dialogue.py" \
  tts_command="python3 /path/to/qwen3_tts.py"
```

The real worker commands are not included because their executable paths, model files, CUDA runtime, and microphone setup belong to the target laptop. Their required stdin/stdout contracts are:

- `NN_VOICE_CAPTURE_COMMAND`: long-running command that writes signed 16-bit little-endian mono PCM at 16 kHz to stdout. It is started on `ptt` and terminated on release.
- `NN_VOICE_STT_COMMAND`: command that reads the captured PCM from stdin and writes one final UTF-8 transcript to stdout.
- `NN_VOICE_DIALOGUE_COMMAND`: command that reads a JSON request containing the bounded Response Context and transcript from stdin, then writes `{"dialogue":"..."}` to stdout.
- `NN_VOICE_TTS_COMMAND`: command that reads JSON containing `engine: "qwen3-tts"`, `model: "Qwen3-TTS-1.7B"`, `voice_id`, `text`, and `sample_rate: 24000`; it must write signed 16-bit little-endian mono PCM at 24 kHz to stdout.

The TTS adapter is intentionally named and validated as Qwen3-TTS 1.7B. Pocket TTS, Piper, and automatic fallback engines are not part of this daemon.

Each command has a bounded 30-second deadline. The dialogue worker receives only the active Subscriber Profile, current goal and Call Premise, Story Beat direction, permitted knowledge, beliefs, Relationship Notes, selected Memories, and at most six recent conversation turns. The approximate dynamic context limit is 3,072 tokens.

## Manual test

1. Start the backend: `just backend`.
2. Start the daemon with smoke workers: `just voice-daemon`. For real workers, use the override command above.
3. Start Odin with `just frontend` and connect Subscriber 0 to the Operator Jack.
4. Hold the PTT control in Odin. The backend forwards `StartPtt`; the daemon captures microphone audio and reports `listening`.
5. Release PTT. The daemon reports `transcribing`, `generating_response`, `synthesizing`, `playing`, and `completed`, while sending RTP/L16 audio to the backend UDP boundary.
6. Confirm the transcript and response in the daemon output, `speaker_active` in the next backend snapshot, and no Routing receipt was created by voice processing.

For an isolated daemon test, type `ptt` and `release` on its stdin instead of using Odin. With the default smoke workers, this exercises the same provider and UDP output path without requiring the GUI or external model files.

The backend UDP port can be changed with `--voice-bind`; pass the matching address as `NN_VOICE_BACKEND_ADDRESS` when invoking `just voice-daemon backend_address=...`.
