#!/usr/bin/env bash
set -euo pipefail

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:-"$HOME/.local/share/north-neeladesh/voice-benchmark"}
export HF_HOME="$benchmark_root/huggingface"
export HF_HUB_OFFLINE=1
export TRANSFORMERS_OFFLINE=1
exec "$benchmark_root/envs/tts-screening/bin/pocket-tts" serve \
  --host 127.0.0.1 --port 18082 --language english
