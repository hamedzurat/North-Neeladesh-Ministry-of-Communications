"""Capture one bounded 16 kHz mono PCM utterance from the local ALSA device."""

import os
import shutil

from .common import fail


SAMPLE_RATE = 16_000


def main() -> int:
    arecord = shutil.which(os.environ.get("NN_ARECORD_BINARY", "arecord"))
    if arecord is None:
        fail("voice capture requires the local ALSA arecord binary")

    device = os.environ.get("NN_VOICE_CAPTURE_DEVICE", "default")
    try:
        max_seconds = float(os.environ.get("NN_VOICE_MAX_UTTERANCE_SECONDS", "15"))
    except ValueError:
        fail("NN_VOICE_MAX_UTTERANCE_SECONDS must be a number")
    if not 0 < max_seconds <= 60:
        fail("NN_VOICE_MAX_UTTERANCE_SECONDS must be between 0 and 60")

    # Exec keeps the daemon's termination signal attached to arecord.
    os.execv(
        arecord,
        [
            arecord,
            "--quiet",
            "--device",
            device,
            "--format=S16_LE",
            "--rate",
            str(SAMPLE_RATE),
            "--channels=1",
            "--file-type=raw",
            "--duration",
            str(max(1, int(max_seconds))),
        ],
    )
    return 0


if __name__ == "__main__":
    main()
