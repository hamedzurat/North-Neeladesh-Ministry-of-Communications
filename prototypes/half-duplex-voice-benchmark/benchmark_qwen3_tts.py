#!/usr/bin/env python3
"""Generate private Qwen3-TTS expressive listening samples."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import soundfile as sf
from qwen_tts import Qwen3TTSModel


PROTOTYPE_DIR = Path(__file__).resolve().parent
INSTRUCTIONS = {
    "urgent": "Speak urgently, with controlled alarm and clear pronunciation.",
    "angry": "Speak with restrained anger and sharp emphasis, without shouting.",
    "quiet": "Speak quietly and cautiously, as if someone may be listening.",
    "hesitation": "Speak hesitantly, with a worried pause before continuing.",
    "interruption": "Start naturally, then cut off abruptly as if interrupted.",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", default="qwen3-tts-0.6b-ryan")
    parser.add_argument("--model", default="Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice")
    parser.add_argument("--speaker", default="Ryan")
    parser.add_argument("--device-map", default="cuda:0")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    lines = json.loads((PROTOTYPE_DIR / "tts_lines.json").read_text())
    started = time.perf_counter()
    model = Qwen3TTSModel.from_pretrained(
        args.model,
        device_map=args.device_map,
        dtype="bfloat16",
    )
    load_seconds = time.perf_counter() - started
    sample_dir = args.results_dir / "listening-samples" / args.engine_label
    sample_dir.mkdir(parents=True, exist_ok=True)

    for line in lines:
        instruction = INSTRUCTIONS.get(line["category"], "Speak naturally and clearly.")
        utterance_started = time.perf_counter()
        audio, sample_rate = model.generate_custom_voice(
            text=line["text"],
            speaker=args.speaker,
            language="English",
            instruct=instruction,
        )
        output = sample_dir / f"{line['id']}.wav"
        sf.write(output, audio[0], sample_rate)
        print(
            f"{line['id']}: {time.perf_counter() - utterance_started:.3f}s "
            f"{output}"
        )

    result = {
        "engine": args.engine_label,
        "model": args.model,
        "speaker": args.speaker,
        "load_seconds": load_seconds,
        "instructions": INSTRUCTIONS,
        "sample_dir": str(sample_dir.resolve()),
    }
    args.results_dir.mkdir(parents=True, exist_ok=True)
    (args.results_dir / f"{args.engine_label}.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print(f"Private listening samples: {sample_dir}")


if __name__ == "__main__":
    main()
