"""Generate one bounded Subscriber response with a local Qwen3 4B model."""

import json
import os
import sys

from .common import fail, prompt_template, world_knowledge
from .ollama import chat

MAX_DIALOGUE_CHARS = 2_000
MAX_TRANSCRIPT_CHARS = 4_000
MAX_OUTPUT_TOKENS = 96


def prompt_for(request: dict[str, object]) -> list[dict[str, str]]:
    context = request.get("context")
    transcript = request.get("transcript")
    if not isinstance(context, dict) or not isinstance(transcript, str) or not transcript.strip():
        fail("dialogue request must contain context and transcript")
    if len(transcript) > MAX_TRANSCRIPT_CHARS:
        fail("dialogue transcript exceeds the bounded turn limit")
    profile = context.get("profile")
    if not isinstance(profile, dict) or not profile.get("name"):
        fail("dialogue request must contain a Subscriber Profile")
    user_context = {key: value for key, value in context.items() if key != "call_guidance"}
    system = prompt_template("dialogue_system.txt").format(
        call_guidance=context.get("call_guidance", ""),
        world_knowledge=world_knowledge(),
        max_dialogue_chars=MAX_DIALOGUE_CHARS,
    )
    user = prompt_template("dialogue_user.txt").format(
        context_json=json.dumps(user_context, ensure_ascii=True, separators=(",", ":")),
        transcript=transcript,
    )
    return [{"role": "system", "content": system}, {"role": "user", "content": user}]


def persistent_main() -> int:
    model = os.environ.get("NN_OLLAMA_MODEL", "qwen3.5:4b")
    for line in sys.stdin.buffer:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            prompt = prompt_for(request)
            content = chat(
                model=model,
                messages=prompt,
                options={
                    "temperature": float(os.environ.get("NN_DIALOGUE_TEMPERATURE", "0.35")),
                    "num_predict": int(os.environ.get("NN_OLLAMA_DIALOGUE_NUM_PREDICT", str(MAX_OUTPUT_TOKENS))),
                },
                keep_alive=600,
            )
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
    raise SystemExit(persistent_main())
