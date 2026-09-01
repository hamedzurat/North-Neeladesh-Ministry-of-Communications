#!/usr/bin/env python3
"""Generate one-actor, five-state Pocket TTS casting samples."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from urllib.parse import urlparse

import numpy as np
import soundfile as sf
import torch
from pocket_tts import TTSModel


STATES = ("neutral", "urgent", "quiet", "angry", "interrupted")
PROTOTYPE_DIR = Path(__file__).resolve().parent


def parse_voice(value: str) -> tuple[str, str]:
    if "=" not in value:
        raise argparse.ArgumentTypeError("expected STATE=VOICE_OR_WAV")
    state, voice = value.split("=", 1)
    if state not in STATES:
        raise argparse.ArgumentTypeError(f"state must be one of: {', '.join(STATES)}")
    if not voice:
        raise argparse.ArgumentTypeError("voice reference cannot be empty")
    return state, voice


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument(
        "--neutral-voice",
        default="alba",
        help="Pocket catalog voice or consented WAV used for the one-actor baseline",
    )
    parser.add_argument(
        "--state-voice",
        action="append",
        type=parse_voice,
        default=[],
        metavar="STATE=VOICE_OR_WAV",
        help="optional directed reference; repeat for urgent, quiet, angry, interrupted",
    )
    parser.add_argument("--language", default="english")
    parser.add_argument("--torch-threads", type=int, default=2)
    return parser.parse_args()


def audio_from_stream(model: TTSModel, voice_state: dict, text: str):
    chunks = []
    for chunk in model.generate_audio_stream(voice_state, text, copy_state=True):
        if hasattr(chunk, "detach"):
            chunk = chunk.detach().cpu().numpy()
        chunks.append(chunk.reshape(-1))
    if not chunks:
        raise RuntimeError("Pocket TTS returned no audio chunks")
    return np.concatenate(chunks)


def main() -> None:
    args = parse_args()
    torch.set_num_threads(args.torch_threads)
    lines = {line["state"]: line for line in json.loads((PROTOTYPE_DIR / "casting_lines.json").read_text())}
    directed = dict(args.state_voice)
    model = TTSModel.load_model(language=args.language)
    states = {state: directed.get(state, args.neutral_voice) for state in STATES}

    for state, voice in states.items():
        parsed = urlparse(voice)
        is_remote = parsed.scheme in {"hf", "http", "https"}
        if not is_remote and ("/" in voice or "\\" in voice) and not Path(voice).exists():
            raise SystemExit(f"voice reference does not exist: {voice}")
        voice_state = model.get_state_for_audio_prompt(voice)
        output_dir = args.results_dir / "casting-samples" / state
        output_dir.mkdir(parents=True, exist_ok=True)
        audio = audio_from_stream(model, voice_state, lines[state]["text"])
        output = output_dir / f"{lines[state]['id']}.wav"
        sf.write(output, audio, model.sample_rate)
        print(f"{state}: {output}")

    print(f"Generated {len(states)} sample(s). Compare neutral with directed references by listening, not by filename.")


if __name__ == "__main__":
    main()
