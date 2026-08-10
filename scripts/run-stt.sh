#!/usr/bin/env bash
set -euo pipefail

benchmark_root=${NN_MVP_VOICE_BENCHMARK_ROOT:-"$HOME/.local/share/north-neeladesh/voice-benchmark"}
exec "$benchmark_root/tools/whisper.cpp-v1.9.2/build-cpu/bin/whisper-server" \
  --model "$benchmark_root/tools/whisper.cpp-v1.9.2/models/ggml-base.en.bin" \
  --host 127.0.0.1 --port 18080 --inference-path /inference --no-timestamps
