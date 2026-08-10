#!/usr/bin/env bash
set -euo pipefail

benchmark_root="$HOME/.local/share/north-neeladesh/voice-benchmark"
stt_port=${NN_MVP_STT_PORT:?Set by just stt}
exec "$benchmark_root/tools/whisper.cpp-v1.9.2/build-cpu/bin/whisper-server" \
  --model "$benchmark_root/tools/whisper.cpp-v1.9.2/models/ggml-base.en.bin" \
  --host 127.0.0.1 --port "$stt_port" --inference-path /inference --no-timestamps
