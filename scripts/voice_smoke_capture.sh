#!/bin/sh

# Emit silence while the daemon owns the PTT capture process.
dd if=/dev/zero bs=2 count=2400 2>/dev/null
while :; do sleep 1; done
