#!/usr/bin/env bash
set -euo pipefail

benchmark_root="$HOME/.local/share/north-neeladesh/voice-benchmark"
llm_port=${NN_MVP_LLM_PORT:?Set by just llm}
exec "$benchmark_root/tools/llama.cpp-b10327-vulkan/llama-server" \
  --model "$benchmark_root/models/qwen3-4b-instruct-2507-q4_k_m/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf" \
  --host 127.0.0.1 --port "$llm_port" --ctx-size 4096 --no-webui
