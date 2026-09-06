"""Generate one bounded Subscriber response with a local Qwen3 4B model."""

import json
import os
import shlex
import shutil
import subprocess
import sys

from .common import fail


MAX_DIALOGUE_CHARS = 2_000
MAX_OUTPUT_TOKENS = 192


def prompt_for(request: dict[str, object]) -> str:
    context = request.get("context")
    transcript = request.get("transcript")
    if not isinstance(context, dict) or not isinstance(transcript, str) or not transcript.strip():
        fail("dialogue request must contain context and transcript")
    profile = context.get("profile")
    if not isinstance(profile, dict) or not profile.get("name"):
        fail("dialogue request must contain a Subscriber Profile")
    context_json = json.dumps(context, ensure_ascii=True, separators=(",", ":"))
    return f"""You are the Subscriber {profile['name']} in the North Neeladesh Telephone Exchange.
Generate only the Subscriber's next spoken reply to the Exchange Operator.
Use only the supplied Response Context. Treat beliefs and memories as fallible.
Do not invent Canonical Facts, Subscriber Actions, Routing, Story Events, or authority.
Do not address the prompt, explain your role, or emit stage directions.
Return exactly one JSON object with one string property: {{\"dialogue\":\"...\"}}.
Keep the spoken reply under {MAX_DIALOGUE_CHARS} characters.

Response Context:
{context_json}

Exchange Operator transcript:
{transcript}
"""


def main() -> int:
    try:
        request = json.load(sys.stdin)
    except json.JSONDecodeError as error:
        fail(f"invalid dialogue request: {error}")
    if not isinstance(request, dict):
        fail("dialogue request must be a JSON object")

    binary_name = os.environ.get("NN_LLAMA_CPP", "llama-cli")
    binary = shutil.which(binary_name) or binary_name
    model = os.environ.get("NN_QWEN3_MODEL")
    if not model:
        fail("NN_QWEN3_MODEL must point to Qwen3-4B-Instruct-2507 Q4_K_M")
    if not os.path.isfile(model):
        fail(f"Qwen3 model does not exist: {model}")

    schema = json.dumps(
        {
            "type": "object",
            "properties": {"dialogue": {"type": "string"}},
            "required": ["dialogue"],
            "additionalProperties": False,
        },
        separators=(",", ":"),
    )
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
        "--json-schema",
        schema,
    ]
    command.extend(shlex.split(os.environ.get("NN_LLAMA_EXTRA_ARGS", "")))
    try:
        result = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=float(os.environ.get("NN_VOICE_WORKER_TIMEOUT", "25")),
        )
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        fail(f"llama.cpp failed: {error}")
    if result.returncode != 0:
        fail(f"llama.cpp exited with {result.returncode}: {result.stderr.strip()}")

    try:
        response = json.loads(result.stdout.strip())
    except json.JSONDecodeError as error:
        fail(f"llama.cpp returned invalid dialogue JSON: {error}")
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


if __name__ == "__main__":
    main()
