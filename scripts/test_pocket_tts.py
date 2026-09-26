"""Synthesize one sentence with a PocketTTS voice embedding."""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
import wave
from pathlib import Path

SAMPLE_RATE = 24_000
OUTPUT_GAIN = 1.5


def load_model():
    """Load the CPU-only PocketTTS model."""
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
    return model, torch


def load_voice_states(model, mode_files: list[Path]) -> dict[Path, object]:
    """Load all voice embeddings so batch synthesis does not reload them."""
    return {mode_file: model.get_state_for_audio_prompt(str(mode_file)) for mode_file in mode_files}


def synthesize(model, text: str, voice_state, torch) -> list[float]:
    """Generate samples using one PocketTTS voice embedding."""
    samples: list[float] = []
    for chunk in model.generate_audio_stream(voice_state, text):
        samples.extend(chunk.detach().to(device="cpu", dtype=torch.float32).flatten().tolist())
    return samples


def write_wav(output_file: Path, samples: list[float]) -> None:
    """Write mono signed 16-bit PCM without playing it."""
    pcm = b"".join(
        struct.pack("<h", round(max(-1.0, min(1.0, sample * OUTPUT_GAIN)) * 32767))
        for sample in samples
    )
    output_file.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(output_file), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(pcm)


def output_directory(text: str) -> Path:
    """Return a safe, readable temporary directory name for the sentence."""
    slug = "-".join(text.split())
    slug = "".join(character for character in slug if character.isalnum() or character in "-_")
    return Path("/tmp/neel") / (slug[:120] or "pocket-tts")


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
    parser.add_argument("sentences", nargs="+", help="one or more sentences to synthesize")
    parser.add_argument("--mode-file", type=Path, help="single PocketTTS voice file to play")
    parser.add_argument(
        "--all",
        action="store_true",
        help="synthesize every .safetensors file in python/.models into /tmp/neel/<sentence>",
    )
    parser.add_argument("--output-dir", type=Path, help="directory for --all output")
    args = parser.parse_args()

    try:
        if args.all and args.mode_file:
            parser.error("--mode-file cannot be used with --all")
        if not args.all and not args.mode_file:
            parser.error("--mode-file is required unless --all is used")
        if not args.all and len(args.sentences) != 1:
            parser.error("only one sentence can be played with --mode-file")

        model, torch = load_model()
        if args.all:
            mode_files = sorted(Path(__file__).resolve().parents[1].joinpath("python", ".models").glob("*.safetensors"))
            if not mode_files:
                raise RuntimeError("no .safetensors files found in python/.models")
            voice_states = load_voice_states(model, mode_files)
            for sentence in args.sentences:
                destination = args.output_dir or output_directory(sentence)
                for mode_file, voice_state in voice_states.items():
                    samples = synthesize(model, sentence, voice_state, torch)
                    if not samples:
                        raise RuntimeError(f"PocketTTS returned empty audio for {mode_file.name}")
                    write_wav(destination / f"{mode_file.stem}.wav", samples)
                print(f"wrote {len(mode_files)} files to {destination}")
            return 0

        if not args.mode_file.is_file():
            raise FileNotFoundError(f"mode file does not exist: {args.mode_file}")
        voice_state = load_voice_states(model, [args.mode_file])[args.mode_file]
        samples = synthesize(model, args.sentences[0], voice_state, torch)
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
