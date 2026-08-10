#!/usr/bin/env bash
set -euo pipefail

audio_path=${1:?Usage: local-stt.sh <captured-wav>}
stt_url=${NN_MVP_STT_URL:-http://127.0.0.1:18080/inference}

[[ -f "$audio_path" ]] || {
  printf 'Captured audio is unavailable: %s\n' "$audio_path" >&2
  exit 1
}

curl --fail-with-body --silent --show-error \
  --max-time 30 \
  --form "file=@${audio_path};type=audio/wav" \
  --form temperature=0.0 \
  --form response_format=json \
  "$stt_url" | jq --raw-output '.text'
