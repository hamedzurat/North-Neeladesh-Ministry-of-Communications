#!/usr/bin/env bash
set -euo pipefail

APP_USER="${SUDO_USER:-${USER:?missing USER}}"
APP_ROOT="${NN_HARDWARE_ROOT:-/home/$APP_USER/Desktop/nn-hardware-frontend}"
VENV="${NN_HARDWARE_VENV:-/home/$APP_USER/venv}"
SERVICE_NAME="north-neeladesh-hardware-frontend"
SERVICE_PATH="/etc/systemd/system/$SERVICE_NAME.service"
RULE_PATH="/etc/udev/rules.d/99-north-neeladesh-leds.rules"
BACKEND_ADDRESS="${NN_BACKEND_ADDRESS:-127.0.0.1:7878}"

if [[ ! -x "$VENV/bin/python" ]]; then
    printf 'Missing Python virtual environment: %s\n' "$VENV" >&2
    exit 1
fi

UV_BIN="${UV_BIN:-}"
if [[ -z "$UV_BIN" ]] && command -v uv >/dev/null 2>&1; then
    UV_BIN="$(command -v uv)"
fi
if [[ -z "$UV_BIN" ]] && [[ -x "$HOME/.local/bin/uv" ]]; then
    UV_BIN="$HOME/.local/bin/uv"
fi
if [[ -z "$UV_BIN" ]]; then
    printf 'uv is required. Install it from https://docs.astral.sh/uv/getting-started/\n' >&2
    exit 1
fi

UV_PROJECT_ENVIRONMENT="$VENV" "$UV_BIN" sync --project "$APP_ROOT" --locked --no-dev

sudo usermod -a -G gpio,i2c,spi "$APP_USER"

printf '%s\n' \
    'SUBSYSTEM=="ws2812-pio-rp1", KERNEL=="leds0", GROUP="gpio", MODE="0660"' \
    | sudo tee "$RULE_PATH" >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger --name-match=leds0 || true

sudo tee "$SERVICE_PATH" >/dev/null <<SERVICE
[Unit]
Description=North Neeladesh Cabinet Frontend
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$APP_USER
SupplementaryGroups=gpio i2c spi
WorkingDirectory=$APP_ROOT
Environment=NN_HARDWARE_MODE=real
Environment=NN_BACKEND_ADDRESS=$BACKEND_ADDRESS
Environment=PYTHONPATH=$APP_ROOT:/home/$APP_USER/Desktop
ExecStart=$VENV/bin/python -m hardware_frontend
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
SERVICE

sudo systemctl daemon-reload
sudo systemctl enable "$SERVICE_NAME.service"

printf '\nInstalled %s.service.\n' "$SERVICE_NAME"
printf 'The service is enabled but not started.\n'
printf 'Start: sudo systemctl start %s.service\n' "$SERVICE_NAME"
printf 'Logs:  journalctl -u %s.service -f\n' "$SERVICE_NAME"
printf 'A new login may be required for the updated GPIO groups.\n'
