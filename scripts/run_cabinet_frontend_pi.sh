#!/usr/bin/env bash
set -euo pipefail

APP_USER="${USER:?missing USER}"
APP_UID="$(id -u)"
APP_ROOT="${NN_CABINET_ROOT:-/home/$APP_USER/Desktop/cabinet-frontend}"
VENV="${NN_HARDWARE_VENV:-/home/$APP_USER/venv}"
SERVICE_NAME="north-neeladesh-cabinet-frontend"

if systemctl is-active --quiet "$SERVICE_NAME.service"; then
    printf 'Stop %s.service before running the frontend manually.\n' "$SERVICE_NAME" >&2
    exit 1
fi

if [[ ! -x "$VENV/bin/python" ]]; then
    printf 'Missing Python environment: %s\n' "$VENV" >&2
    exit 1
fi

cd "$APP_ROOT"
exec env \
    PYTHONPATH="$APP_ROOT:/home/$APP_USER/Desktop" \
    XDG_RUNTIME_DIR="/run/user/$APP_UID" \
    NN_BACKEND_HOST="${NN_BACKEND_HOST:?set NN_BACKEND_HOST first}" \
    NN_PAIR_SCAN_INTERVAL="${NN_PAIR_SCAN_INTERVAL:-0.5}" \
    NN_EPAPER_ROTATION="${NN_EPAPER_ROTATION:-90}" \
    "$VENV/bin/python" -m cabinet_frontend "$@"
