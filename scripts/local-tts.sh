#!/usr/bin/env bash
set -euo pipefail

voice_configuration=${1:?Usage: local-tts.sh <voice-configuration> <output-wav>}
output_path=${2:?Usage: local-tts.sh <voice-configuration> <output-wav>}
tts_url="http://127.0.0.1:${NN_MVP_TTS_PORT:?Set by just backend}/tts"

text=$(cat)
[[ -n "$text" ]] || {
  printf 'Pocket TTS received no response text.\n' >&2
  exit 1
}

raw_output_path="${output_path}.pocket.wav"
cleanup() {
  rm -f -- "$raw_output_path"
}
trap cleanup EXIT

curl --fail-with-body --silent --show-error \
  --max-time 30 \
  --form "text=${text}" \
  "$tts_url" > "$raw_output_path"

case "$voice_configuration" in
  pocket-tts:nila-low-warm) filter='asetrate=21800,aresample=24000' ;;
  pocket-tts:sorin-clear-neutral) filter='asetrate=24600,aresample=24000' ;;
  pocket-tts:arun-brisk-mid) filter='asetrate=24000,atempo=1.08,aresample=24000' ;;
  pocket-tts:leela-measured-low) filter='asetrate=20800,atempo=0.94,aresample=24000' ;;
  *)
    printf 'Unknown local voice configuration: %s\n' "$voice_configuration" >&2
    exit 1
    ;;
esac

ffmpeg -v error -y -i "$raw_output_path" -filter:a "$filter" "$output_path"
