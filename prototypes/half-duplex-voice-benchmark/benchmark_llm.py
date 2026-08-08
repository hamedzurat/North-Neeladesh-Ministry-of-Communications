#!/usr/bin/env python3
"""Benchmark a persistent OpenAI-compatible local dialogue server."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import re
import subprocess
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


PROTOTYPE_DIR = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-url", default="http://127.0.0.1:18081/v1/chat/completions")
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
    parser.add_argument("--server-pid", type=int)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--seed", type=int, default=2701)
    parser.add_argument("--temperature", type=float, default=0.2)
    parser.add_argument("--max-tokens", type=int, default=160)
    parser.add_argument("--timeout", type=float, default=120)
    return parser.parse_args()


def percentile(values: list[float], proportion: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(proportion * len(ordered)) - 1)]


def process_rss_kib(pid: int | None) -> int | None:
    if pid is None:
        return None
    try:
        status = Path(f"/proc/{pid}/status").read_text()
    except FileNotFoundError:
        return None
    match = re.search(r"^VmRSS:\s+(\d+)\s+kB$", status, re.MULTILINE)
    return int(match.group(1)) if match else None


def gpu_memory_used_mib() -> int | None:
    try:
        result = subprocess.run(
            [
                "nvidia-smi",
                "--query-gpu=memory.used",
                "--format=csv,noheader,nounits",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return int(result.stdout.splitlines()[0].strip())
    except (FileNotFoundError, subprocess.CalledProcessError, ValueError, IndexError):
        return None


def stream_completion(
    server_url: str,
    system_prompt: str,
    response_context: str,
    seed: int,
    temperature: float,
    max_tokens: int,
    timeout: float,
) -> tuple[str, float, float, dict]:
    payload = json.dumps(
        {
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": "RESPONSE CONTEXT\n" + response_context},
            ],
            "stream": True,
            "stream_options": {"include_usage": True},
            "temperature": temperature,
            "seed": seed,
            "max_tokens": max_tokens,
        }
    ).encode()
    request = urllib.request.Request(
        server_url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    started = time.perf_counter()
    chunks = []
    time_to_first_speech = None
    usage = {}
    with urllib.request.urlopen(request, timeout=timeout) as response:
        for raw_line in response:
            line = raw_line.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            event = json.loads(line.removeprefix("data: "))
            if event.get("usage"):
                usage = event["usage"]
            choices = event.get("choices") or []
            content = choices[0].get("delta", {}).get("content", "") if choices else ""
            if not content:
                continue
            chunks.append(content)
            accumulated = "".join(chunks)
            speech_match = re.search(r"^SPEECH:\s*(\S)", accumulated, re.MULTILINE)
            if speech_match and time_to_first_speech is None:
                time_to_first_speech = time.perf_counter() - started
    total = time.perf_counter() - started
    if time_to_first_speech is None:
        raise RuntimeError("Response never produced a SPEECH value")
    return "".join(chunks), time_to_first_speech, total, usage


def parse_protocol(output: str) -> tuple[str | None, object, object, list[str]]:
    pattern = re.compile(
        r"\ASPEECH:\s*(?P<speech>[^\n]+)\n"
        r"QUERY:\s*(?P<query>[^\n]+)\n"
        r"ACTION:\s*(?P<action>[^\n]+)\s*\Z"
    )
    match = pattern.match(output.strip())
    if not match:
        return None, None, None, ["invalid three-line protocol"]
    errors = []

    def parse_slot(name: str) -> object:
        value = match.group(name).strip()
        if value.lower() == "none":
            return None
        try:
            parsed = json.loads(value)
        except json.JSONDecodeError:
            errors.append(f"invalid {name} JSON")
            return None
        if not isinstance(parsed, dict):
            errors.append(f"{name} must be a JSON object")
        return parsed

    return match.group("speech").strip(), parse_slot("query"), parse_slot("action"), errors


def evaluate(case: dict, output: str) -> dict:
    speech, query, action, errors = parse_protocol(output)
    normalized = output.casefold()
    for phrase in case.get("required_all", []):
        if phrase.casefold() not in normalized:
            errors.append(f"missing required phrase: {phrase}")
    required_any = case.get("required_any", [])
    if required_any and not any(phrase.casefold() in normalized for phrase in required_any):
        errors.append("missing every required-any phrase")
    for phrase in case.get("forbidden_all", []):
        if phrase.casefold() in normalized:
            errors.append(f"included forbidden phrase: {phrase}")
    if query != case.get("expected_query"):
        errors.append(f"query mismatch: expected {case.get('expected_query')!r}, got {query!r}")
    if action != case.get("expected_action"):
        errors.append(f"action mismatch: expected {case.get('expected_action')!r}, got {action!r}")
    if speech is not None and len(speech.split()) > 45:
        errors.append("speech exceeds 45 words")
    return {"passed": not errors, "errors": errors, "speech": speech, "query": query, "action": action}


def main() -> None:
    args = parse_args()
    if args.runs < 1:
        raise SystemExit("--runs must be at least 1")
    system_prompt = (PROTOTYPE_DIR / "llm_system_prompt.txt").read_text().strip()
    cases = json.loads((PROTOTYPE_DIR / "llm_cases.json").read_text())

    # Warm up generation and the server's prompt cache path.
    stream_completion(
        args.server_url,
        system_prompt,
        cases[0]["response_context"],
        args.seed,
        0.0,
        24,
        args.timeout,
    )

    observations = []
    for run_number in range(1, args.runs + 1):
        for case in cases:
            output, first_speech, total, usage = stream_completion(
                args.server_url,
                system_prompt,
                case["response_context"],
                args.seed + run_number - 1,
                args.temperature,
                args.max_tokens,
                args.timeout,
            )
            evaluation = evaluate(case, output)
            observation = {
                "run": run_number,
                "case_id": case["id"],
                "category": case["category"],
                "time_to_first_speech_text_seconds": first_speech,
                "total_generation_seconds": total,
                "usage": usage,
                "output": output,
                "evaluation": evaluation,
                "server_rss_kib": process_rss_kib(args.server_pid),
                "gpu_memory_used_mib": gpu_memory_used_mib(),
            }
            observations.append(observation)
            print(
                f"{args.engine_label} run {run_number}/{args.runs} {case['id']}: "
                f"first={first_speech:.3f}s total={total:.3f}s "
                f"hard_gate={'pass' if evaluation['passed'] else 'FAIL'}"
            )
            if evaluation["errors"]:
                print("  " + "; ".join(evaluation["errors"]))

    first_values = [item["time_to_first_speech_text_seconds"] for item in observations]
    total_values = [item["total_generation_seconds"] for item in observations]
    rss_values = [item["server_rss_kib"] for item in observations if item["server_rss_kib"]]
    vram_values = [item["gpu_memory_used_mib"] for item in observations if item["gpu_memory_used_mib"] is not None]
    passed = sum(1 for item in observations if item["evaluation"]["passed"])
    completion_tokens = sum(item["usage"].get("completion_tokens", 0) for item in observations)
    summary = {
        "engine": args.engine_label,
        "runs": args.runs,
        "cases": len(cases),
        "observations": len(observations),
        "hard_gate_passes": passed,
        "hard_gate_pass_rate": passed / len(observations),
        "time_to_first_speech_text_seconds": {
            "p50": percentile(first_values, 0.50),
            "p95": percentile(first_values, 0.95),
            "max": max(first_values),
        },
        "total_generation_seconds": {
            "p50": percentile(total_values, 0.50),
            "p95": percentile(total_values, 0.95),
            "max": max(total_values),
        },
        "completion_tokens_per_second": completion_tokens / sum(total_values),
        "server_peak_rss_kib": max(rss_values) if rss_values else None,
        "gpu_peak_memory_used_mib": max(vram_values) if vram_values else None,
    }
    result = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "server_url": args.server_url,
            "server_pid": args.server_pid,
            "temperature": args.temperature,
            "seed": args.seed,
            "max_tokens": args.max_tokens,
        },
        "summary": summary,
        "system_prompt": system_prompt,
        "cases": cases,
        "observations": observations,
    }
    args.results_dir.mkdir(parents=True, exist_ok=True)
    result_path = args.results_dir / f"{args.engine_label}.json"
    result_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    print(f"Detailed private result: {result_path}")


if __name__ == "__main__":
    main()
