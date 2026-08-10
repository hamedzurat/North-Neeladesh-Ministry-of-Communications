#!/usr/bin/env bash
set -euo pipefail

voice_assets="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/assets/pocket-tts-voices"
voice_snapshot="${HF_HOME:?HF_HOME must be set before preparing Pocket TTS voices}/hub/models--kyutai--pocket-tts-without-voice-cloning/snapshots/e041936c75475d350b405bc870bcf7c22da4e9e6/languages/english/embeddings"

mkdir -p "$voice_snapshot"
for voice in anna alba charles vera; do
  source="$voice_assets/$voice.safetensors"
  if [[ ! -s "$source" ]]; then
    echo "Missing bundled Pocket TTS profile: $source" >&2
    exit 1
  fi
  cp "$source" "$voice_snapshot/$voice.safetensors"
done
