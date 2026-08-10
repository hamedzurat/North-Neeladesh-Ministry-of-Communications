#!/usr/bin/env bash
set -euo pipefail

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:?Set NN_MVP_VOICE_BENCHMARK_ROOT to the prepared local voice workspace.}
llama_cli="$benchmark_root/tools/llama.cpp-b10327-vulkan/llama-cli"
model="$benchmark_root/models/qwen3-4b-instruct-2507-q4_k_m/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf"

[[ -x "$llama_cli" && -f "$model" ]] || {
  printf 'Prepared llama.cpp Qwen runtime or model is unavailable.\n' >&2
  exit 1
}

prompt=$(cat)
raw_output=$("$llama_cli" \
  --model "$model" \
  --simple-io \
  --single-turn \
  --no-display-prompt \
  --no-show-timings \
  --temp 0.2 \
  --n-predict 70 \
  --prompt "$prompt" 2>/dev/null)

printf '%s\n' "$raw_output" | awk '
  /^> / { collecting = 1; next }
  /^Exiting/ { exit }
  collecting && NF { print }
'
