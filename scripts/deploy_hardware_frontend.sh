#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PI_HOST="${PI_HOST:-taki@192.168.1.11}"
REMOTE_ROOT="${REMOTE_ROOT:-/home/taki/Desktop/nn-hardware-frontend}"

if ! command -v rsync >/dev/null 2>&1; then
    printf 'rsync is required for deployment.\n' >&2
    exit 1
fi

ssh "$PI_HOST" "mkdir -p '$REMOTE_ROOT'"
ssh "$PI_HOST" "mkdir -p '$REMOTE_ROOT/scripts'"
rsync --archive --compress \
    --exclude='__pycache__' \
    --exclude='.venv' \
    "$ROOT_DIR/python/cabinet_frontend/" \
    "$PI_HOST:$REMOTE_ROOT/"
rsync --archive --compress \
    "$ROOT_DIR/scripts/setup_hardware_frontend_pi.sh" \
    "$PI_HOST:$REMOTE_ROOT/scripts/setup_hardware_frontend_pi.sh"

printf 'Copied hardware frontend to %s:%s\n' "$PI_HOST" "$REMOTE_ROOT"
printf 'Next: ssh %s %s/scripts/setup_hardware_frontend_pi.sh\n' "$PI_HOST" "$REMOTE_ROOT"
