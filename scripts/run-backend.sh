#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:-"$HOME/.local/share/north-neeladesh/voice-benchmark"}
if [[ ! -d "$benchmark_root" ]]; then
  printf 'Prepared local voice workspace is unavailable: %s\n' "$benchmark_root" >&2
  exit 1
fi
export NN_MVP_VOICE_BENCHMARK_ROOT="$benchmark_root"
export NN_MVP_STT_COMMAND="${NN_MVP_STT_COMMAND:-$project_root/scripts/local-stt.sh}"
export NN_MVP_DIALOGUE_COMMAND="${NN_MVP_DIALOGUE_COMMAND:-$project_root/scripts/local-dialogue.sh}"
export NN_MVP_TTS_COMMAND="${NN_MVP_TTS_COMMAND:-$project_root/scripts/local-tts.sh}"

for asset in \
  "$benchmark_root/tools/whisper.cpp-v1.9.2/build-cpu/bin/whisper-cli" \
  "$benchmark_root/tools/whisper.cpp-v1.9.2/models/ggml-base.en.bin" \
  "$benchmark_root/tools/llama.cpp-b10327-vulkan/llama-cli" \
  "$benchmark_root/models/qwen3-4b-instruct-2507-q4_k_m/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf" \
  "$benchmark_root/envs/tts-screening/bin/pocket-tts"; do
  [[ -e "$asset" ]] || {
    printf 'Required local voice asset is unavailable: %s\n' "$asset" >&2
    exit 1
  }
done

exec cargo run --manifest-path backend/Cargo.toml --bin backend
