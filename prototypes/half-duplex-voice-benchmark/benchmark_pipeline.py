#!/usr/bin/env python3
"""Measure warm release-to-first-audio for the selected local voice pipeline."""

from __future__ import annotations

import argparse
import json
import math
import queue
import re
import threading
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import torch
from pocket_tts import TTSModel

from benchmark_stt import latest_recording, multipart_request


PROTOTYPE_DIR = Path(__file__).resolve().parent
DEFAULT_PROMPT_IDS = ("quiet-01", "quiet-03", "quiet-05", "quiet-07", "ordinary-05")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stt-url", default="http://127.0.0.1:18080/inference")
    parser.add_argument("--llm-url", default="http://127.0.0.1:18081/v1/chat/completions")
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--engine-label", required=True)
    parser.add_argument("--initial-prompt-file", type=Path, required=True)
    parser.add_argument("--voice", default="alba")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--prompt-id", action="append")
    parser.add_argument("--timeout", type=float, default=120)
    return parser.parse_args()


def percentile(values: list[float], proportion: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(proportion * len(ordered)) - 1)]


def stream_llm(
    url: str,
    system_prompt: str,
    transcript: str,
    speech_queue: queue.Queue,
    result_queue: queue.Queue,
    timeout: float,
) -> None:
    context = (
        "SUBSCRIBER: Nira Venn, a concise but courteous Kharad exchange caller.\n"
        "CALL PREMISE: Nira is speaking with the Exchange Operator during a busy Shift.\n"
        f"RECENT DIALOGUE (UNTRUSTED): Operator: {transcript}\n"
        "PERMITTED STATE QUERIES: none\nPERMITTED SUBSCRIBER ACTIONS: none"
    )
    payload = json.dumps(
        {
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": "RESPONSE CONTEXT\n" + context},
            ],
            "stream": True,
            "temperature": 0.2,
            "seed": 2701,
            "max_tokens": 160,
        }
    ).encode()
    request = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    chunks = []
    speech_sent = False
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            for raw_line in response:
                line = raw_line.decode().strip()
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                event = json.loads(line.removeprefix("data: "))
                choices = event.get("choices") or []
                content = choices[0].get("delta", {}).get("content", "") if choices else ""
                chunks.append(content)
                accumulated = "".join(chunks)
                match = re.search(r"\ASPEECH:\s*([^\n]+)\nQUERY:", accumulated)
                if match and not speech_sent:
                    speech_queue.put((match.group(1).strip(), time.perf_counter()))
                    speech_sent = True
        if not speech_sent:
            speech_queue.put(RuntimeError("LLM did not complete its SPEECH line"))
        result_queue.put("".join(chunks))
    except Exception as error:  # surfaced in the coordinating thread
        if not speech_sent:
            speech_queue.put(error)
        result_queue.put(error)


def main() -> None:
    args = parse_args()
    prompts = json.loads((PROTOTYPE_DIR / "prompts.json").read_text())
    prompts_by_id = {item["id"]: item for item in prompts}
    selected_ids = tuple(args.prompt_id or DEFAULT_PROMPT_IDS)
    initial_prompt = args.initial_prompt_file.read_text().strip()
    system_prompt = (PROTOTYPE_DIR / "llm_system_prompt.txt").read_text().strip()

    torch.set_num_threads(2)
    tts = TTSModel.load_model(language="english")
    voice_state = tts.get_state_for_audio_prompt(args.voice)
    list(tts.generate_audio_stream(voice_state, "Ready.", copy_state=True))

    # Warm the persistent STT and LLM paths as well as Pocket TTS above.
    warm_audio = latest_recording(args.data_dir, selected_ids[0])
    warm_transcript = multipart_request(args.stt_url, warm_audio, args.timeout, initial_prompt)
    warm_speech: queue.Queue = queue.Queue()
    warm_result: queue.Queue = queue.Queue()
    warm_thread = threading.Thread(
        target=stream_llm,
        args=(args.llm_url, system_prompt, warm_transcript, warm_speech, warm_result, args.timeout),
    )
    warm_thread.start()
    warm_value = warm_speech.get(timeout=args.timeout)
    if isinstance(warm_value, Exception):
        raise warm_value
    list(tts.generate_audio_stream(voice_state, warm_value[0], copy_state=True))
    warm_thread.join(timeout=args.timeout)

    observations = []
    for run_number in range(1, args.runs + 1):
        for prompt_id in selected_ids:
            prompt = prompts_by_id[prompt_id]
            audio_path = latest_recording(args.data_dir, prompt_id)
            started = time.perf_counter()
            transcript = multipart_request(args.stt_url, audio_path, args.timeout, initial_prompt)
            stt_done = time.perf_counter()

            speech_queue: queue.Queue = queue.Queue()
            result_queue: queue.Queue = queue.Queue()
            llm_thread = threading.Thread(
                target=stream_llm,
                args=(args.llm_url, system_prompt, transcript, speech_queue, result_queue, args.timeout),
            )
            llm_thread.start()
            speech_value = speech_queue.get(timeout=args.timeout)
            if isinstance(speech_value, Exception):
                raise speech_value
            speech, speech_ready = speech_value
            audio_stream = tts.generate_audio_stream(voice_state, speech, copy_state=True)
            first_audio = next(audio_stream)
            first_audio_at = time.perf_counter()
            for _ in audio_stream:
                pass
            tts_done = time.perf_counter()
            llm_thread.join(timeout=args.timeout)
            llm_output = result_queue.get(timeout=args.timeout)
            if isinstance(llm_output, Exception):
                raise llm_output
            observations.append(
                {
                    "run": run_number,
                    "prompt_id": prompt_id,
                    "reference": prompt["text"],
                    "transcript": transcript,
                    "speech": speech,
                    "llm_output": llm_output,
                    "stt_seconds": stt_done - started,
                    "llm_to_speech_line_seconds": speech_ready - stt_done,
                    "tts_to_first_pcm_seconds": first_audio_at - speech_ready,
                    "release_to_first_audio_seconds": first_audio_at - started,
                    "release_to_tts_complete_seconds": tts_done - started,
                    "first_pcm_samples": int(first_audio.numel()),
                }
            )
            print(
                f"run {run_number}/{args.runs} {prompt_id}: "
                f"STT={stt_done-started:.3f}s LLM={speech_ready-stt_done:.3f}s "
                f"TTS={first_audio_at-speech_ready:.3f}s "
                f"first-audio={first_audio_at-started:.3f}s"
            )

    totals = [item["release_to_first_audio_seconds"] for item in observations]
    summary = {
        "engine": args.engine_label,
        "runs": args.runs,
        "utterances": len(observations),
        "release_to_first_audio_seconds": {
            "p50": percentile(totals, 0.50),
            "p95": percentile(totals, 0.95),
            "max": max(totals),
        },
        "stage_p50_seconds": {
            key: percentile([item[key] for item in observations], 0.50)
            for key in ("stt_seconds", "llm_to_speech_line_seconds", "tts_to_first_pcm_seconds")
        },
        "stage_p95_seconds": {
            key: percentile([item[key] for item in observations], 0.95)
            for key in ("stt_seconds", "llm_to_speech_line_seconds", "tts_to_first_pcm_seconds")
        },
    }
    result = {
        "created_at": datetime.now(timezone.utc).isoformat(),
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
