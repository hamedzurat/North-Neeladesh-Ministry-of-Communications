"""Synthesize one sentence with a PocketTTS voice embedding."""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
from pathlib import Path

SAMPLE_RATE = 24_000
OUTPUT_GAIN = 1.5


def synthesize(text: str, mode_file: Path) -> list[float]:
    """Generate samples using one PocketTTS voice embedding."""
    if not mode_file.is_file():
        raise FileNotFoundError(f"mode file does not exist: {mode_file}")

    try:
        import torch
        from pocket_tts import TTSModel
    except ImportError as error:
        raise RuntimeError(
            "PocketTTS dependencies are unavailable; run this script with "
            "'uv run --project python'"
        ) from error

    model = TTSModel.load_model()
    if model.device.type != "cpu":
        raise RuntimeError(f"PocketTTS must run on CPU, got {model.device}")
    if model.sample_rate != SAMPLE_RATE:
        raise RuntimeError(f"PocketTTS returned unsupported sample rate {model.sample_rate}")

    voice_state = model.get_state_for_audio_prompt(str(mode_file))
    samples: list[float] = []
    for chunk in model.generate_audio_stream(voice_state, text):
        samples.extend(chunk.detach().to(device="cpu", dtype=torch.float32).flatten().tolist())
    return samples


def play(samples: list[float]) -> None:
    """Play mono signed 16-bit PCM through the default PipeWire device."""
    pcm = b"".join(
        struct.pack("<h", round(max(-1.0, min(1.0, sample * OUTPUT_GAIN)) * 32767))
        for sample in samples
    )
    try:
        subprocess.run(
            [
                "pw-play",
                "--raw",
                "--format",
                "s16",
                "--rate",
                str(SAMPLE_RATE),
                "--channels",
                "1",
                "-",
            ],
            input=pcm,
            check=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError("'pw-play' is not installed; install PipeWire's CLI tools to play audio") from error
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"audio playback failed with exit code {error.returncode}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sentence", help="sentence to synthesize")
    parser.add_argument("mode_file", type=Path, help="PocketTTS voice .safetensors file")
    args = parser.parse_args()

    try:
        samples = synthesize(args.sentence, args.mode_file)
        if not samples:
            raise RuntimeError("PocketTTS returned empty audio")
        play(samples)
    except (FileNotFoundError, RuntimeError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"played ({len(samples) / SAMPLE_RATE:.2f}s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
