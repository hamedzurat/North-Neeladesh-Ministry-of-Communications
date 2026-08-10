#!/usr/bin/env bash
set -euo pipefail

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:-"$HOME/.local/share/north-neeladesh/voice-benchmark"}
exec "$benchmark_root/tools/llama.cpp-b10327-vulkan/llama-server" \
  --model "$benchmark_root/models/qwen3-4b-instruct-2507-q4_k_m/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf" \
  --host 127.0.0.1 --port 18081 --ctx-size 4096 --no-webui
