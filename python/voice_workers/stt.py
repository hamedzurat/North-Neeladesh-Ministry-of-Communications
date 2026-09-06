"""Run whisper.cpp against signed 16-bit mono 16 kHz PCM from stdin."""

import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import wave

from .common import WHISPER_BINARY, WHISPER_MODEL, fail


SAMPLE_RATE = 16_000


def main() -> int:
    pcm = sys.stdin.buffer.read()
    if not pcm or len(pcm) % 2:
        fail("STT input must be non-empty signed 16-bit PCM")

    binary_name = os.environ.get("NN_WHISPER_CPP", str(WHISPER_BINARY))
    binary = shutil.which(binary_name) or binary_name
    model = os.environ.get("NN_WHISPER_MODEL", str(WHISPER_MODEL))
    if not os.path.isfile(model):
        fail(f"whisper model does not exist: {model}")

    with tempfile.NamedTemporaryFile(suffix=".wav") as audio:
        with wave.open(audio, "wb") as writer:
            writer.setnchannels(1)
            writer.setsampwidth(2)
            writer.setframerate(SAMPLE_RATE)
            writer.writeframes(pcm)
        audio.flush()

        command = [
            binary,
            "--model",
            model,
            "--file",
            audio.name,
            "--language",
            "en",
            "--no-timestamps",
            "--no-prints",
        ]
        command.extend(shlex.split(os.environ.get("NN_WHISPER_EXTRA_ARGS", "")))
        try:
            result = subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=float(os.environ.get("NN_VOICE_WORKER_TIMEOUT", "25")),
            )
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            fail(f"whisper.cpp failed: {error}")

    if result.returncode != 0:
        fail(f"whisper.cpp exited with {result.returncode}: {result.stderr.strip()}")

    transcript_lines = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if line and not line.startswith(("whisper_", "main:")):
            transcript_lines.append(line)
    transcript = " ".join(transcript_lines).strip()
    if not transcript:
        fail("whisper.cpp returned an empty transcript")
    sys.stdout.write(transcript)
    return 0


if __name__ == "__main__":
    main()
