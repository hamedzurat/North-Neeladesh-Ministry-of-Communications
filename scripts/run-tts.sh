#!/usr/bin/env bash
set -euo pipefail

benchmark_root="$HOME/.local/share/north-neeladesh/voice-benchmark"
tts_port=${NN_MVP_TTS_PORT:?Set by just tts}
export HF_HOME="$benchmark_root/huggingface"
export HF_HUB_OFFLINE=1
export TRANSFORMERS_OFFLINE=1
"$(dirname "$0")/prepare-pocket-tts-voices.sh"
exec "$benchmark_root/envs/tts-screening/bin/pocket-tts" serve \
  --host 127.0.0.1 --port "$tts_port" --language english
