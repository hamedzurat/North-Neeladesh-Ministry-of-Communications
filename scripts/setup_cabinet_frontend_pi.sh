#!/usr/bin/env bash
set -euo pipefail

APP_USER="${SUDO_USER:-${USER:?missing USER}}"
APP_UID="$(id -u "$APP_USER")"
APP_ROOT="${NN_CABINET_ROOT:-/home/$APP_USER/Desktop/cabinet-frontend}"
VENV="${NN_HARDWARE_VENV:-/home/$APP_USER/venv}"
SERVICE_NAME="north-neeladesh-cabinet-frontend"
SERVICE_PATH="/etc/systemd/system/$SERVICE_NAME.service"
RULE_PATH="/etc/udev/rules.d/99-north-neeladesh-cabinet-leds.rules"
PRINTER_RULE_PATH="/etc/udev/rules.d/98-north-neeladesh-cabinet-printer.rules"
AUDIO_ROOT="${NN_AUDIO_ROOT:-$APP_ROOT/assets/stories}"
BACKEND_ADDRESS="${NN_BACKEND_ADDRESS:-127.0.0.1:7878}"
if [[ -n "${NN_BACKEND_HOST:-}" ]]; then
    BACKEND_ADDRESS="${NN_BACKEND_ADDRESS:-$NN_BACKEND_HOST:7878}"
fi
BACKEND_HOST="${BACKEND_ADDRESS%:*}"
VOICE_BACKEND_ADDRESS="${NN_VOICE_BACKEND_ADDRESS:-$BACKEND_HOST:7879}"

if [[ ! -d "$AUDIO_ROOT" ]] || ! find "$AUDIO_ROOT" -type f \( -name '*.wav' -o -name '*.m4a' \) -print -quit | grep -q .; then
    printf 'No authored audio files found under %s. Deploy the assets or run `just story-audio` first.\n' \
        "$AUDIO_ROOT" >&2
    exit 1
fi

if [[ -f "$APP_ROOT/audio-manifest.sha256" ]]; then
    if ! (cd "$APP_ROOT" && sha256sum --check audio-manifest.sha256); then
        printf 'Authored audio verification failed under %s.\n' "$AUDIO_ROOT" >&2
        exit 1
    fi
fi

if [[ -z "${NN_VOICE_CAPTURE_COMMAND:-}" ]] && ! command -v pw-record >/dev/null 2>&1; then
    printf 'pw-record is required for the default PipeWire voice capture path.\n' >&2
    exit 1
fi
if [[ -z "${NN_VOICE_PLAYBACK_COMMAND:-}" ]] && ! command -v aplay >/dev/null 2>&1; then
    printf 'aplay is required for the default voice playback path.\n' >&2
    exit 1
fi

if command -v apt-get >/dev/null 2>&1 && ! ldconfig -p 2>/dev/null | grep -q 'libportaudio'; then
    printf 'Installing PortAudio runtime for callback-based capture.\n'
    sudo apt-get install -y libportaudio2 libasound2-plugins
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

sudo usermod -a -G gpio,i2c,spi,lp "$APP_USER"

printf '%s\n' \
    'SUBSYSTEM=="ws2812-pio-rp1", KERNEL=="leds0", GROUP="gpio", MODE="0660"' \
    | sudo tee "$RULE_PATH" >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger --name-match=leds0 || true

printf '%s\n' \
    'KERNEL=="lp[0-9]*", GROUP="lp", MODE="0660"' \
    | sudo tee "$PRINTER_RULE_PATH" >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --name-match=lp0 2>/dev/null || true

sudo tee "$SERVICE_PATH" >/dev/null <<SERVICE
[Unit]
Description=North Neeladesh Cabinet Frontend
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$APP_USER
SupplementaryGroups=gpio i2c spi audio lp
WorkingDirectory=$APP_ROOT
Environment=NN_BACKEND_ADDRESS=$BACKEND_ADDRESS
Environment="NN_VOICE_BACKEND_ADDRESS=$VOICE_BACKEND_ADDRESS"
Environment=XDG_RUNTIME_DIR=/run/user/$APP_UID
Environment=NN_PAIR_SCAN_INTERVAL=${NN_PAIR_SCAN_INTERVAL:-0.5}
Environment="NN_VOICE_CAPTURE_COMMAND=${NN_VOICE_CAPTURE_COMMAND:-}"
Environment="NN_VOICE_PLAYBACK_COMMAND=${NN_VOICE_PLAYBACK_COMMAND:-}"
Environment=PYTHONPATH=$APP_ROOT:/home/$APP_USER/Desktop
ExecStart=$VENV/bin/python -m cabinet_frontend
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
SERVICE

sudo systemctl daemon-reload
sudo systemctl disable --now north-neeladesh-voice-daemon.service 2>/dev/null || true
sudo systemctl disable --now north-neeladesh-voice-relay.service 2>/dev/null || true
sudo systemctl enable "$SERVICE_NAME.service"

printf '\nInstalled %s.service.\n' "$SERVICE_NAME"
printf 'Authored audio assets: %s\n' "$AUDIO_ROOT"
printf 'The service is enabled but not started.\n'
printf 'Start: sudo systemctl start %s.service\n' "$SERVICE_NAME"
printf 'Logs:  journalctl -u %s.service -f\n' "$SERVICE_NAME"
printf 'Voice capture/control/playback runs inside %s.service.\n' "$SERVICE_NAME"
printf 'A new login may be required for the updated GPIO groups.\n'
