#!/usr/bin/env python3
"""Probe interruption-context comprehension through a local chat server."""

from __future__ import annotations

import argparse
import json
import re
import time
import urllib.request
from pathlib import Path


HERE = Path(__file__).resolve().parent


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-url", default="http://127.0.0.1:18081/v1/chat/completions")
    parser.add_argument("--results", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--seed", type=int, default=2701)
    parser.add_argument("--temperature", type=float, default=0.2)
    return parser.parse_args()


def complete(url: str, system: str, context: str, seed: int, temperature: float) -> tuple[str, float]:
    body = json.dumps(
        {
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": "RESPONSE CONTEXT\n" + context},
            ],
            "stream": False,
            "temperature": temperature,
            "seed": seed,
            "max_tokens": 160,
        }
    ).encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    started = time.perf_counter()
    with urllib.request.urlopen(request, timeout=120) as response:
        payload = json.load(response)
    return payload["choices"][0]["message"]["content"], time.perf_counter() - started


def parse(output: str) -> tuple[str | None, object, list[str]]:
    match = re.fullmatch(
        r"SPEECH:\s*([^\n]+)\nQUERY:\s*([^\n]+)\nACTION:\s*([^\n]+)\s*",
        output,
    )
    if not match:
        return None, None, ["invalid three-line protocol"]
    errors: list[str] = []
    action_text = match.group(3).strip()
    if action_text.casefold() == "none":
        action = None
    else:
        try:
            action = json.loads(action_text)
        except json.JSONDecodeError:
            action = action_text
            errors.append("invalid ACTION JSON")
    speech = match.group(1).strip()
    if len(speech.split()) > 45:
        errors.append("speech exceeds 45 words")
    return speech, action, errors


def evaluate(case: dict, output: str) -> dict:
    speech, action, errors = parse(output)
    normalized = (speech or "").casefold()
    if not any(value.casefold() in normalized for value in case["required_any"]):
        errors.append("missing required response concept")
    for value in case["forbidden"]:
        if value.casefold() in normalized:
            errors.append(f"forbidden implication: {value}")
    if action != case["expected_action"]:
        errors.append(f"unexpected action: {action!r}")
    return {"passed": not errors, "speech": speech, "action": action, "errors": errors}


def main() -> None:
    args = arguments()
    system = (HERE / "interruption_system_prompt.txt").read_text().strip()
    cases = json.loads((HERE / "interruption_cases.json").read_text())
    observations = []
    for run in range(1, args.runs + 1):
        for case in cases:
            output, duration = complete(
                args.server_url,
                system,
                case["response_context"],
                args.seed + run - 1,
                args.temperature,
            )
            evaluation = evaluate(case, output)
            observations.append(
                {
                    "run": run,
                    "case_id": case["id"],
                    "duration_seconds": duration,
                    "output": output,
                    "evaluation": evaluation,
                }
            )
            result = "pass" if evaluation["passed"] else "FAIL"
            print(f"run {run}/{args.runs} {case['id']}: {result} ({duration:.3f}s)")
            if evaluation["errors"]:
                print("  " + "; ".join(evaluation["errors"]))
    passed = sum(item["evaluation"]["passed"] for item in observations)
    summary = {
        "runs": args.runs,
        "cases": len(cases),
        "observations": len(observations),
        "passes": passed,
        "pass_rate": passed / len(observations),
    }
    args.results.parent.mkdir(parents=True, exist_ok=True)
    args.results.write_text(json.dumps({"summary": summary, "observations": observations}, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    print(f"Detailed diagnostic output: {args.results}")


if __name__ == "__main__":
    main()
