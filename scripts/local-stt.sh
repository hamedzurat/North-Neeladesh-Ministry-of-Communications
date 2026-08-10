#!/usr/bin/env bash
set -euo pipefail

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:?Set NN_MVP_VOICE_BENCHMARK_ROOT to the prepared local voice workspace.}
audio_path=${1:?Usage: local-stt.sh <captured-wav>}
whisper_cli="$benchmark_root/tools/whisper.cpp-v1.9.2/build-cpu/bin/whisper-cli"
model="$benchmark_root/tools/whisper.cpp-v1.9.2/models/ggml-base.en.bin"

[[ -x "$whisper_cli" && -f "$model" && -f "$audio_path" ]] || {
  printf 'Prepared whisper.cpp base.en runtime, model, or captured audio is unavailable.\n' >&2
  exit 1
}

"$whisper_cli" --model "$model" --file "$audio_path" --no-timestamps --no-prints 2>/dev/null
