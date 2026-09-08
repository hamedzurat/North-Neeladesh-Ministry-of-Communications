#!/usr/bin/env bash
set -euo pipefail

PI_HOST="${PI_HOST:-taki@192.168.1.34}"

ssh -tt "$PI_HOST" \
    'sudo systemctl status --no-pager north-neeladesh-hardware-frontend.service north-neeladesh-voice-daemon.service'
