"""Generate one bounded Subscriber response with a local Qwen3 4B model."""

import json
import os
import shlex
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

from .common import DIALOGUE_MODEL, LLAMA_BINARY, dialogue_prompt_template, fail, worker_timeout

MAX_DIALOGUE_CHARS = 2_000
MAX_TRANSCRIPT_CHARS = 4_000
MAX_OUTPUT_TOKENS = 96
DEFAULT_LLAMA_ARGS = "--ctx-size 4096 --n-gpu-layers 99 --no-warmup"


def prompt_for(request: dict[str, object]) -> str:
    context = request.get("context")
    transcript = request.get("transcript")
    if not isinstance(context, dict) or not isinstance(transcript, str) or not transcript.strip():
        fail("dialogue request must contain context and transcript")
    if len(transcript) > MAX_TRANSCRIPT_CHARS:
        fail("dialogue transcript exceeds the bounded turn limit")
    profile = context.get("profile")
    if not isinstance(profile, dict) or not profile.get("name"):
        fail("dialogue request must contain a Subscriber Profile")
    context_json = json.dumps(context, ensure_ascii=True, separators=(",", ":"))
    caller_place = context.get("caller_place", "the exchange")
    requested_place = context.get("requested_place", "an unknown place")
    caller_name = profile["name"]
    return dialogue_prompt_template().format(
        caller_name=caller_name,
        caller_place=caller_place,
        requested_place=requested_place,
        max_dialogue_chars=MAX_DIALOGUE_CHARS,
        context_json=context_json,
        transcript=transcript,
    )


def main() -> int:
    if "--persistent" in sys.argv:
        return persistent_main()
    try:
        request = json.load(sys.stdin)
    except json.JSONDecodeError as error:
        fail(f"invalid dialogue request: {error}")
    if not isinstance(request, dict):
        fail("dialogue request must be a JSON object")

    binary_name = os.environ.get("NN_LLAMA_CPP", str(LLAMA_BINARY))
    binary = shutil.which(binary_name) or binary_name
    model = os.environ.get("NN_QWEN3_MODEL", str(DIALOGUE_MODEL))
    if not os.path.isfile(model):
        fail(f"Qwen3 model does not exist: {model}")

    command = [
        binary,
        "--model",
        model,
        "--prompt",
        prompt_for(request),
        "--n-predict",
        str(MAX_OUTPUT_TOKENS),
        "--temp",
        os.environ.get("NN_DIALOGUE_TEMPERATURE", "0.35"),
        "--no-display-prompt",
        "--single-turn",
        "--simple-io",
        "--no-show-timings",
    ]
    command.extend(shlex.split(os.environ.get("NN_LLAMA_EXTRA_ARGS", DEFAULT_LLAMA_ARGS)))
    try:
        result = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=worker_timeout(),
        )
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        fail(f"llama.cpp failed: {error}")
    if result.returncode != 0:
        fail(f"llama.cpp exited with {result.returncode}: {result.stderr.strip()}")

    decoder = json.JSONDecoder()
    response = None
    for index, character in enumerate(result.stdout):
        if character != "{":
            continue
        try:
            candidate, _ = decoder.raw_decode(result.stdout[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(candidate, dict) and set(candidate) == {"dialogue"}:
            response = candidate
    if response is None:
        fail("llama.cpp returned invalid dialogue JSON")
    if not isinstance(response, dict) or set(response) != {"dialogue"}:
        fail("dialogue JSON must contain only the dialogue property")
    dialogue = response["dialogue"]
    if not isinstance(dialogue, str) or not dialogue.strip():
        fail("dialogue JSON did not contain non-empty dialogue")
    dialogue = dialogue.strip()
    if len(dialogue) > MAX_DIALOGUE_CHARS:
        fail("dialogue exceeded the bounded turn limit")
    json.dump({"dialogue": dialogue}, sys.stdout, ensure_ascii=False, separators=(",", ":"))
    return 0


def persistent_main() -> int:
    base_url = os.environ.get("NN_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/")
    model = os.environ.get("NN_OLLAMA_MODEL", "qwen3.5:4b")
    for line in sys.stdin.buffer:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            prompt = prompt_for(request)
            body = json.dumps(
                {
                    "model": model,
                    "messages": [{"role": "user", "content": prompt}],
                    "think": False,
                    "format": "json",
                    "stream": False,
                    "keep_alive": -1,
                    "options": {
                        "temperature": float(os.environ.get("NN_DIALOGUE_TEMPERATURE", "0.35")),
                        "num_predict": MAX_OUTPUT_TOKENS,
                    },
                }
            ).encode()
            http_request = urllib.request.Request(
                f"{base_url}/api/chat",
                data=body,
                headers={"Content-Type": "application/json"},
            )
            with urllib.request.urlopen(http_request, timeout=worker_timeout()) as response:
                result = json.load(response)
            content = result["message"]["content"]
            if not isinstance(content, str):
                fail("Ollama returned invalid dialogue content")
            dialogue = json.loads(content).get("dialogue")
            if not isinstance(dialogue, str) or not dialogue.strip():
                fail("Ollama returned invalid dialogue JSON")
            dialogue = dialogue.strip()
            if len(dialogue) > MAX_DIALOGUE_CHARS:
                fail("dialogue exceeded the bounded turn limit")
            sys.stdout.write(json.dumps({"dialogue": dialogue}, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        except Exception as error:  # noqa: BLE001 - worker reports runtime failures
            fail(f"persistent Ollama dialogue synthesis failed: {error}")
if __name__ == "__main__":
    main()
