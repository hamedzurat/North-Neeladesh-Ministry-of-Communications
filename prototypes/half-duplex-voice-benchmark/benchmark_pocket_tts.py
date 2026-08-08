#!/usr/bin/env python3
"""Measure Pocket TTS streaming latency and produce private listening samples."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import re
import time
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from pocket_tts import TTSModel


PROTOTYPE_DIR = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
    parser.add_argument("--voice", default="alba")
    parser.add_argument("--language", default="english")
    parser.add_argument("--torch-threads", type=int, default=2)
    parser.add_argument("--runs", type=int, default=5)
    return parser.parse_args()


def percentile(values: list[float], proportion: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(proportion * len(ordered)) - 1)]


def process_rss_kib() -> int | None:
    match = re.search(
        r"^VmRSS:\s+(\d+)\s+kB$",
        Path(f"/proc/{os.getpid()}/status").read_text(),
        re.MULTILINE,
    )
    return int(match.group(1)) if match else None


def as_numpy(audio: object) -> np.ndarray:
    if hasattr(audio, "detach"):
        audio = audio.detach().cpu().numpy()
    return np.asarray(audio, dtype=np.float32).reshape(-1)


def synthesize(
    model: TTSModel, voice_state: dict, text: str
) -> tuple[float, float, list[np.ndarray]]:
    started = time.perf_counter()
    chunks = []
    first_chunk_seconds = None
    for audio in model.generate_audio_stream(voice_state, text, copy_state=True):
        if first_chunk_seconds is None:
            first_chunk_seconds = time.perf_counter() - started
        chunks.append(as_numpy(audio))
    total_seconds = time.perf_counter() - started
    if first_chunk_seconds is None:
        raise RuntimeError("Pocket TTS returned no audio chunks")
    return first_chunk_seconds, total_seconds, chunks


def main() -> None:
    args = parse_args()
    if args.runs < 1:
        raise SystemExit("--runs must be at least 1")
    torch.set_num_threads(args.torch_threads)
    lines = json.loads((PROTOTYPE_DIR / "tts_lines.json").read_text())

    load_started = time.perf_counter()
    model = TTSModel.load_model(language=args.language)
    load_seconds = time.perf_counter() - load_started
    voice_started = time.perf_counter()
    voice_state = model.get_state_for_audio_prompt(args.voice)
    voice_load_seconds = time.perf_counter() - voice_started

    # Discard a complete warm-up inference after loading the voice state.
    synthesize(model, voice_state, lines[0]["text"])

    sample_rate = model.sample_rate
    sample_dir = args.results_dir / "listening-samples" / args.engine_label
    sample_dir.mkdir(parents=True, exist_ok=True)
    observations = []
    for run_number in range(1, args.runs + 1):
        for line in lines:
            first_chunk, total, chunks = synthesize(
                model, voice_state, line["text"]
            )
            audio = np.concatenate(chunks)
            audio_seconds = len(audio) / sample_rate
            if run_number == 1:
                sf.write(sample_dir / f"{line['id']}.wav", audio, sample_rate)
            observations.append(
                {
                    "run": run_number,
                    "line_id": line["id"],
                    "category": line["category"],
                    "text": line["text"],
                    "time_to_first_pcm_seconds": first_chunk,
                    "total_synthesis_seconds": total,
                    "audio_seconds": audio_seconds,
                    "first_chunk_audio_seconds": len(chunks[0]) / sample_rate,
                    "real_time_factor": total / audio_seconds,
                    "process_rss_kib": process_rss_kib(),
                }
            )
            print(
                f"{args.engine_label} run {run_number}/{args.runs} {line['id']}: "
                f"first={first_chunk:.3f}s total={total:.3f}s "
                f"audio={audio_seconds:.3f}s chunks={len(chunks)}"
            )

    first_pcm = [item["time_to_first_pcm_seconds"] for item in observations]
    totals = [item["total_synthesis_seconds"] for item in observations]
    rtfs = [item["real_time_factor"] for item in observations]
    rss_values = [
        item["process_rss_kib"]
        for item in observations
        if item["process_rss_kib"] is not None
    ]
    summary = {
        "engine": args.engine_label,
        "voice": args.voice,
        "runs": args.runs,
        "utterances": len(observations),
        "model_load_seconds": load_seconds,
        "voice_load_seconds": voice_load_seconds,
        "time_to_first_pcm_seconds": {
            "p50": percentile(first_pcm, 0.50),
            "p95": percentile(first_pcm, 0.95),
            "max": max(first_pcm),
        },
        "total_synthesis_seconds": {
            "p50": percentile(totals, 0.50),
            "p95": percentile(totals, 0.95),
            "max": max(totals),
        },
        "real_time_factor": {
            "p50": percentile(rtfs, 0.50),
            "p95": percentile(rtfs, 0.95),
        },
        "process_peak_rss_kib": max(rss_values) if rss_values else None,
    }
    result = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "pocket_tts": version("pocket-tts"),
            "torch": torch.__version__,
            "torch_threads": args.torch_threads,
        },
        "summary": summary,
        "observations": observations,
    }
    args.results_dir.mkdir(parents=True, exist_ok=True)
    result_path = args.results_dir / f"{args.engine_label}.json"
    result_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    print(f"Detailed private result: {result_path}")
    print(f"Private listening samples: {sample_dir}")


if __name__ == "__main__":
    main()
