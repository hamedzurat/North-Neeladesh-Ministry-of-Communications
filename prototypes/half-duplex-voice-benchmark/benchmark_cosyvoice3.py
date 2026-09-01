#!/usr/bin/env python3
"""Generate private CosyVoice3 streaming listening samples."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import soundfile as sf
import torch
from cosyvoice.cli.cosyvoice import AutoModel


PROTOTYPE_DIR = Path(__file__).resolve().parent
DEFAULT_LINE_IDS = (
    "tts-01", "tts-03", "tts-05", "tts-06", "tts-07", "tts-09",
    "tts-11", "tts-12", "tts-13", "tts-14", "tts-19", "tts-20",
)
PROMPTS = {
    "urgent": "[breath]Speak urgently, with controlled alarm and clear pronunciation.",
    "angry": "Speak with restrained anger and sharp emphasis, without shouting.",
    "quiet": "[breath]Speak quietly and cautiously, as if someone may be listening.",
    "hesitation": "Speak hesitantly, with a worried pause before continuing.",
    "interruption": "Start naturally, then cut off abruptly as if interrupted.",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--engine-label", default="cosyvoice3-0.5b-zero-shot")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    lines = json.loads((PROTOTYPE_DIR / "tts_lines.json").read_text())
    lines_by_id = {line["id"]: line for line in lines}
    started = time.perf_counter()
    model = AutoModel(model_dir=str(args.model_dir))
    load_seconds = time.perf_counter() - started
    sample_dir = args.results_dir / "listening-samples" / args.engine_label
    sample_dir.mkdir(parents=True, exist_ok=True)
    observations = []

    for line_id in DEFAULT_LINE_IDS:
        line = lines_by_id[line_id]
        instruction = PROMPTS.get(line["category"], "Speak naturally and clearly.")
        # Cross-lingual inference needs an explicit language token; without it,
        # English text is commonly interpreted using the model's default language.
        text = f"<|en|>You are a helpful assistant. {instruction}<|endofprompt|>{line['text']}"
        request_started = time.perf_counter()
        chunks = []
        first_chunk_seconds = None
        for item in model.inference_cross_lingual(text, str(args.reference), stream=True):
            if first_chunk_seconds is None:
                first_chunk_seconds = time.perf_counter() - request_started
            chunks.append(item["tts_speech"].detach().cpu().numpy().reshape(-1))
        if first_chunk_seconds is None or not chunks:
            raise RuntimeError(f"CosyVoice3 returned no audio for {line_id}")
        audio = __import__("numpy").concatenate(chunks)
        output = sample_dir / f"{line_id}.wav"
        sf.write(output, audio, model.sample_rate)
        total_seconds = time.perf_counter() - request_started
        observations.append({
            "line_id": line_id,
            "category": line["category"],
            "text": line["text"],
            "instruction": instruction,
            "time_to_first_pcm_seconds": first_chunk_seconds,
            "total_synthesis_seconds": total_seconds,
            "audio_seconds": len(audio) / model.sample_rate,
            "chunks": len(chunks),
            "cuda_allocated_bytes": torch.cuda.memory_allocated() if torch.cuda.is_available() else None,
            "cuda_reserved_bytes": torch.cuda.memory_reserved() if torch.cuda.is_available() else None,
        })
        print(f"{line_id}: first={first_chunk_seconds:.3f}s total={total_seconds:.3f}s chunks={len(chunks)}")

    result = {
        "engine": args.engine_label,
        "model_dir": str(args.model_dir.resolve()),
        "reference": str(args.reference.resolve()),
        "load_seconds": load_seconds,
        "observations": observations,
        "sample_dir": str(sample_dir.resolve()),
    }
    args.results_dir.mkdir(parents=True, exist_ok=True)
    (args.results_dir / f"{args.engine_label}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"Private listening samples: {sample_dir}")


if __name__ == "__main__":
    main()
