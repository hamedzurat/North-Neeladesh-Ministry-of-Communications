#!/usr/bin/env python3
"""Benchmark faster-whisper with the same corpus and scoring as whisper.cpp."""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import subprocess
import time
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path

from faster_whisper import WhisperModel

from benchmark_stt import (
    PROTOTYPE_DIR,
    edit_distance,
    latest_recording,
    normalize,
    percentile,
    wav_duration,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--download-root", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
    parser.add_argument("--model", default="distil-large-v3")
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--compute-type", default="int8")
    parser.add_argument("--cpu-threads", type=int, default=8)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--initial-prompt-file", type=Path)
    return parser.parse_args()


def process_rss_kib() -> int | None:
    match = re.search(
        r"^VmRSS:\s+(\d+)\s+kB$",
        Path(f"/proc/{os.getpid()}/status").read_text(),
        re.MULTILINE,
    )
    return int(match.group(1)) if match else None


def process_gpu_memory_mib() -> int | None:
    query = subprocess.run(
        [
            "nvidia-smi",
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
    )
    if query.returncode != 0:
        return None
    for line in query.stdout.splitlines():
        fields = [field.strip() for field in line.split(",")]
        if len(fields) == 2 and fields[0] == str(os.getpid()):
            return int(fields[1])
    return None


def transcribe(
    model: WhisperModel, audio_path: Path, initial_prompt: str | None
) -> str:
    segments, _ = model.transcribe(
        str(audio_path),
        language="en",
        beam_size=5,
        condition_on_previous_text=False,
        initial_prompt=initial_prompt,
        vad_filter=False,
    )
    return " ".join(segment.text.strip() for segment in segments).strip()


def main() -> None:
    args = parse_args()
    if args.runs < 1:
        raise SystemExit("--runs must be at least 1")
    prompts = json.loads((PROTOTYPE_DIR / "prompts.json").read_text())
    recordings = {
        prompt["id"]: latest_recording(args.data_dir, prompt["id"])
        for prompt in prompts
    }
    initial_prompt = (
        args.initial_prompt_file.read_text().strip()
        if args.initial_prompt_file
        else None
    )
    args.download_root.mkdir(parents=True, exist_ok=True)
    model = WhisperModel(
        args.model,
        device=args.device,
        compute_type=args.compute_type,
        cpu_threads=args.cpu_threads,
        num_workers=1,
        download_root=str(args.download_root),
    )

    # Discard one complete inference to warm runtime allocations and caches.
    transcribe(model, recordings[prompts[0]["id"]], initial_prompt)

    observations = []
    for run_number in range(1, args.runs + 1):
        for prompt in prompts:
            audio_path = recordings[prompt["id"]]
            started = time.perf_counter()
            transcript = transcribe(model, audio_path, initial_prompt)
            elapsed = time.perf_counter() - started
            reference_words = normalize(prompt["text"])
            transcript_words = normalize(transcript)
            critical = {
                phrase: " ".join(normalize(phrase)) in " ".join(transcript_words)
                for phrase in prompt["critical"]
            }
            observations.append(
                {
                    "run": run_number,
                    "prompt_id": prompt["id"],
                    "condition": prompt["condition"],
                    "reference": prompt["text"],
                    "transcript": transcript,
                    "latency_seconds": elapsed,
                    "audio_seconds": wav_duration(audio_path),
                    "word_errors": edit_distance(reference_words, transcript_words),
                    "reference_words": len(reference_words),
                    "critical": critical,
                    "process_rss_kib": process_rss_kib(),
                    "process_gpu_memory_mib": process_gpu_memory_mib(),
                }
            )
            print(
                f"{args.engine_label} run {run_number}/{args.runs} "
                f"{prompt['id']}: {elapsed:.3f}s — {transcript}"
            )

    latencies = [item["latency_seconds"] for item in observations]
    real_time_factors = [
        item["latency_seconds"] / item["audio_seconds"] for item in observations
    ]
    total_word_errors = sum(item["word_errors"] for item in observations)
    total_reference_words = sum(item["reference_words"] for item in observations)
    critical_results = [
        passed
        for item in observations
        for passed in item["critical"].values()
    ]
    rss_values = [
        item["process_rss_kib"]
        for item in observations
        if item["process_rss_kib"] is not None
    ]
    gpu_memory_values = [
        item["process_gpu_memory_mib"]
        for item in observations
        if item["process_gpu_memory_mib"] is not None
    ]
    summary = {
        "engine": args.engine_label,
        "model": args.model,
        "device": args.device,
        "compute_type": args.compute_type,
        "runs": args.runs,
        "utterances": len(observations),
        "corpus_wer": total_word_errors / total_reference_words,
        "critical_phrase_accuracy": sum(critical_results) / len(critical_results),
        "latency_seconds": {
            "p50": percentile(latencies, 0.50),
            "p95": percentile(latencies, 0.95),
            "max": max(latencies),
        },
        "real_time_factor": {
            "p50": percentile(real_time_factors, 0.50),
            "p95": percentile(real_time_factors, 0.95),
        },
        "process_peak_rss_kib": max(rss_values) if rss_values else None,
        "process_peak_gpu_memory_mib": (
            max(gpu_memory_values) if gpu_memory_values else None
        ),
    }
    result = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "faster_whisper": version("faster-whisper"),
            "ctranslate2": version("ctranslate2"),
            "data_dir": str(args.data_dir.resolve()),
            "initial_prompt": initial_prompt,
        },
        "summary": summary,
        "observations": observations,
    }
    args.results_dir.mkdir(parents=True, exist_ok=True)
    result_path = args.results_dir / f"{args.engine_label}.json"
    result_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    print(f"Detailed private result: {result_path}")


if __name__ == "__main__":
    main()
