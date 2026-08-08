#!/usr/bin/env python3
"""Benchmark a persistent whisper.cpp-compatible HTTP inference server."""

from __future__ import annotations

import argparse
import json
import math
import platform
import re
import time
import uuid
import wave
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen


PROTOTYPE_DIR = Path(__file__).resolve().parent
DIGIT_WORDS = {
    "0": "zero",
    "1": "one",
    "2": "two",
    "3": "three",
    "4": "four",
    "5": "five",
    "6": "six",
    "7": "seven",
    "8": "eight",
    "9": "nine",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-url", required=True)
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
    parser.add_argument("--initial-prompt-file", type=Path)
    parser.add_argument("--server-pid", type=int)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=60.0)
    return parser.parse_args()


def normalize(text: str) -> list[str]:
    expanded = re.sub(
        r"\d+",
        lambda match: " " + " ".join(DIGIT_WORDS[digit] for digit in match[0]) + " ",
        text.lower(),
    )
    return re.findall(r"[a-z]+", expanded)


def edit_distance(reference: list[str], hypothesis: list[str]) -> int:
    previous = list(range(len(hypothesis) + 1))
    for ref_index, ref_word in enumerate(reference, start=1):
        current = [ref_index]
        for hyp_index, hyp_word in enumerate(hypothesis, start=1):
            current.append(
                min(
                    previous[hyp_index] + 1,
                    current[hyp_index - 1] + 1,
                    previous[hyp_index - 1] + (ref_word != hyp_word),
                )
            )
        previous = current
    return previous[-1]


def percentile(values: list[float], proportion: float) -> float:
    ordered = sorted(values)
    index = max(0, math.ceil(proportion * len(ordered)) - 1)
    return ordered[index]


def wav_duration(path: Path) -> float:
    with wave.open(str(path), "rb") as audio:
        return audio.getnframes() / audio.getframerate()


def server_rss_kib(pid: int | None) -> int | None:
    if pid is None:
        return None
    status_path = Path(f"/proc/{pid}/status")
    if not status_path.exists():
        return None
    match = re.search(r"^VmRSS:\s+(\d+)\s+kB$", status_path.read_text(), re.MULTILINE)
    return int(match.group(1)) if match else None


def multipart_request(
    url: str, audio_path: Path, timeout: float, initial_prompt: str | None
) -> str:
    boundary = f"----voice-benchmark-{uuid.uuid4().hex}"
    audio = audio_path.read_bytes()
    parts: list[bytes] = []

    def add_field(name: str, value: str) -> None:
        parts.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                value.encode(),
                b"\r\n",
            ]
        )

    parts.extend(
        [
            f"--{boundary}\r\n".encode(),
            (
                'Content-Disposition: form-data; name="file"; '
                f'filename="{audio_path.name}"\r\n'
            ).encode(),
            b"Content-Type: audio/wav\r\n\r\n",
            audio,
            b"\r\n",
        ]
    )
    add_field("temperature", "0.0")
    add_field("temperature_inc", "0.2")
    if initial_prompt:
        add_field("prompt", initial_prompt)
    add_field("response_format", "json")
    parts.append(f"--{boundary}--\r\n".encode())
    request = Request(
        url,
        data=b"".join(parts),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    with urlopen(request, timeout=timeout) as response:
        payload = json.load(response)
    return payload["text"].strip()


def latest_recording(data_dir: Path, prompt_id: str) -> Path:
    recordings = sorted((data_dir / "recordings").glob(f"{prompt_id}_*.wav"))
    if not recordings:
        raise FileNotFoundError(f"No WAV recording found for {prompt_id}")
    return recordings[-1]


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

    # One discarded request ensures model allocation and runtime caches are warm.
    multipart_request(
        args.server_url,
        recordings[prompts[0]["id"]],
        args.timeout,
        initial_prompt,
    )

    observations = []
    for run_number in range(1, args.runs + 1):
        for prompt in prompts:
            audio_path = recordings[prompt["id"]]
            started = time.perf_counter()
            transcript = multipart_request(
                args.server_url, audio_path, args.timeout, initial_prompt
            )
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
                    "server_rss_kib": server_rss_kib(args.server_pid),
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
        item["server_rss_kib"]
        for item in observations
        if item["server_rss_kib"] is not None
    ]
    summary = {
        "engine": args.engine_label,
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
        "server_peak_rss_kib": max(rss_values) if rss_values else None,
    }
    result = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "server_url": args.server_url,
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
