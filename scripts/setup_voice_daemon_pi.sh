#!/usr/bin/env bash
set -euo pipefail

APP_USER="${SUDO_USER:-${USER:?missing USER}}"
APP_UID="$(id -u "$APP_USER")"
APP_ROOT="${NN_VOICE_ROOT:-/home/$APP_USER/Desktop/nn-voice-daemon}"
BACKEND_ADDRESS="${NN_VOICE_BACKEND_ADDRESS:-127.0.0.1:7879}"
CAPTURE_COMMAND="${NN_VOICE_CAPTURE_COMMAND:-}"
PLAYBACK_COMMAND="${NN_VOICE_PLAYBACK_COMMAND:-}"
PLAYBACK_GAIN="${NN_VOICE_PLAYBACK_GAIN:-1.0}"
SERVICE_NAME="north-neeladesh-voice-daemon"
SERVICE_PATH="/etc/systemd/system/$SERVICE_NAME.service"

if [[ ! -x "$APP_ROOT/target/release/exchange-voice-daemon" ]]; then
    printf 'Missing release binary: %s\n' "$APP_ROOT/target/release/exchange-voice-daemon" >&2
    exit 1
fi

sudo usermod -a -G audio "$APP_USER"

sudo tee "$SERVICE_PATH" >/dev/null <<SERVICE
[Unit]
Description=North Neeladesh Voice Daemon
After=network-online.target sound.target
Wants=network-online.target

[Service]
Type=simple
User=$APP_USER
SupplementaryGroups=audio
WorkingDirectory=$APP_ROOT
Environment="NN_VOICE_BACKEND_ADDRESS=$BACKEND_ADDRESS"
Environment="NN_VOICE_CAPTURE_COMMAND=$CAPTURE_COMMAND"
Environment="NN_VOICE_PLAYBACK_COMMAND=$PLAYBACK_COMMAND"
Environment="NN_VOICE_PLAYBACK_GAIN=$PLAYBACK_GAIN"
Environment="XDG_RUNTIME_DIR=/run/user/$APP_UID"
Environment="PULSE_SERVER=unix:/run/user/$APP_UID/pulse/native"
ExecStart=$APP_ROOT/target/release/exchange-voice-daemon
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
