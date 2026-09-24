#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PI_HOST="${PI_HOST:-taki@192.168.1.34}"
REMOTE_ROOT="${REMOTE_ROOT:-/home/taki/Desktop/cabinet-frontend}"
BACKEND_HOST="${BACKEND_HOST:-}"
AUDIO_ROOT="$ROOT_DIR/assets/stories"
METADATA_DIR="$(mktemp -d)"
trap 'rm -rf "$METADATA_DIR"' EXIT

if [[ ! -d "$AUDIO_ROOT" ]] || ! find "$AUDIO_ROOT" -type f \( -name '*.wav' -o -name '*.m4a' \) -print -quit | grep -q .; then
    printf 'No authored audio files found under %s. Generate them with `just story-audio` first.\n' \
        "$AUDIO_ROOT" >&2
    exit 1
fi

if ! command -v rsync >/dev/null 2>&1; then
    printf 'rsync is required for deployment.\n' >&2
    exit 1
fi

ssh "$PI_HOST" "mkdir -p '$REMOTE_ROOT'"
ssh "$PI_HOST" "mkdir -p '$REMOTE_ROOT/scripts'"
ssh "$PI_HOST" "mkdir -p '$REMOTE_ROOT/assets/stories'"
rsync --archive --compress \
    --exclude='__pycache__' \
    --exclude='.venv' \
    "$ROOT_DIR/python/cabinet_frontend/" \
    "$PI_HOST:$REMOTE_ROOT/"
rsync --archive --compress \
    "$ROOT_DIR/python/cabinet_frontend/pyproject.toml" \
    "$ROOT_DIR/python/cabinet_frontend/uv.lock" \
    "$PI_HOST:$REMOTE_ROOT/"
rsync --archive --compress \
    "$ROOT_DIR/scripts/setup_cabinet_frontend_pi.sh" \
    "$PI_HOST:$REMOTE_ROOT/scripts/setup_cabinet_frontend_pi.sh"
rsync --archive --compress \
    --delete \
    "$AUDIO_ROOT/" \
    "$PI_HOST:$REMOTE_ROOT/assets/stories/"

git -C "$ROOT_DIR" rev-parse HEAD > "$METADATA_DIR/.deployment-version"
(
    cd "$ROOT_DIR"
    find assets/stories -type f \( -name '*.wav' -o -name '*.m4a' \) -print0 \
        | sort -z \
        | xargs -0 sha256sum
) > "$METADATA_DIR/audio-manifest.sha256"
rsync --archive --compress \
    "$METADATA_DIR/.deployment-version" \
    "$METADATA_DIR/audio-manifest.sha256" \
    "$PI_HOST:$REMOTE_ROOT/"

printf 'Copied cabinet frontend to %s:%s\n' "$PI_HOST" "$REMOTE_ROOT"
printf 'Copied authored audio assets to %s:%s/assets/stories\n' "$PI_HOST" "$REMOTE_ROOT"
if [[ -n "$BACKEND_HOST" ]]; then
    printf 'Next: ssh %s NN_BACKEND_HOST=%s %s/scripts/setup_cabinet_frontend_pi.sh\n' \
        "$PI_HOST" "$BACKEND_HOST" "$REMOTE_ROOT"
else
    printf 'Next: ssh %s NN_BACKEND_HOST=<LAPTOP_WIFI_IP> %s/scripts/setup_cabinet_frontend_pi.sh\n' \
        "$PI_HOST" "$REMOTE_ROOT"
fi
