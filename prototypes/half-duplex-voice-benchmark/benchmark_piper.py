#!/usr/bin/env python3
"""Measure Piper streaming latency and produce private listening samples."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import re
import time
import wave
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path

from piper import PiperVoice


PROTOTYPE_DIR = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
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


def synthesize(voice: PiperVoice, text: str) -> tuple[float, float, list[object]]:
    started = time.perf_counter()
    chunks = []
    first_chunk_seconds = None
    for chunk in voice.synthesize(text):
        if first_chunk_seconds is None:
            first_chunk_seconds = time.perf_counter() - started
        chunks.append(chunk)
    total_seconds = time.perf_counter() - started
    if first_chunk_seconds is None:
        raise RuntimeError("Piper returned no audio chunks")
    return first_chunk_seconds, total_seconds, chunks


def save_wav(path: Path, chunks: list[object]) -> float:
    first = chunks[0]
    audio = b"".join(chunk.audio_int16_bytes for chunk in chunks)
    with wave.open(str(path), "wb") as wav_file:
        wav_file.setframerate(first.sample_rate)
        wav_file.setsampwidth(first.sample_width)
        wav_file.setnchannels(first.sample_channels)
        wav_file.writeframes(audio)
    bytes_per_second = first.sample_rate * first.sample_width * first.sample_channels
    return len(audio) / bytes_per_second


def main() -> None:
    args = parse_args()
    if args.runs < 1:
        raise SystemExit("--runs must be at least 1")
    lines = json.loads((PROTOTYPE_DIR / "tts_lines.json").read_text())
    load_started = time.perf_counter()
    voice = PiperVoice.load(args.model, config_path=args.config)
    load_seconds = time.perf_counter() - load_started

    # Discard a complete warm-up inference.
    synthesize(voice, lines[0]["text"])

    sample_dir = args.results_dir / "listening-samples" / args.engine_label
    sample_dir.mkdir(parents=True, exist_ok=True)
    observations = []
    for run_number in range(1, args.runs + 1):
        for line in lines:
            first_chunk, total, chunks = synthesize(voice, line["text"])
            if run_number == 1:
                audio_seconds = save_wav(sample_dir / f"{line['id']}.wav", chunks)
            else:
                first = chunks[0]
                byte_count = sum(len(chunk.audio_int16_bytes) for chunk in chunks)
                audio_seconds = byte_count / (
                    first.sample_rate * first.sample_width * first.sample_channels
                )
            first_chunk_audio_seconds = len(chunks[0].audio_int16_bytes) / (
                chunks[0].sample_rate
                * chunks[0].sample_width
                * chunks[0].sample_channels
            )
            observations.append(
                {
                    "run": run_number,
                    "line_id": line["id"],
                    "category": line["category"],
                    "text": line["text"],
                    "time_to_first_pcm_seconds": first_chunk,
                    "total_synthesis_seconds": total,
                    "audio_seconds": audio_seconds,
                    "first_chunk_audio_seconds": first_chunk_audio_seconds,
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
        "runs": args.runs,
        "utterances": len(observations),
        "model_load_seconds": load_seconds,
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
            "piper_tts": version("piper-tts"),
            "model": str(args.model.resolve()),
            "config": str(args.config.resolve()) if args.config else None,
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
