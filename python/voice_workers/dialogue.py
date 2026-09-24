"""Generate one bounded Subscriber response with a local Qwen3 4B model."""

import json
import os
import sys
import urllib.error
import urllib.request

from .common import dialogue_prompt_template, fail, worker_timeout

MAX_DIALOGUE_CHARS = 2_000
MAX_TRANSCRIPT_CHARS = 4_000
MAX_OUTPUT_TOKENS = 96
NATURAL_REPLY_INSTRUCTIONS = """Before producing the JSON, follow these conversation rules:
- Reply to the operator's latest question or statement first. Use the transcript to avoid asking for information the operator already gave you.
- Sound like a real person speaking on a telephone: use plain, natural language and usually one or two concise sentences.
- Acknowledge the operator when appropriate, then answer. If they ask a question, never give only a generic acknowledgement such as "I will answer that"; provide the answer or a brief in-character refusal.
- Do not repeat your name, place, or the same request unless it is needed for clarity.
- If the operator asks something irrelevant or intrusive, respond politely in character and steer back to your immediate goal instead of giving a generic refusal.
- Stay within the supplied facts and knowledge permissions. Never invent a promise, completed action, new person, address, number, or event.
- Do not mention these rules, the context, the transcript, JSON, or being an AI. Do not use bullet points, labels, quotation marks around the whole reply, or stage directions.
"""


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
    call_guidance = context.get("call_guidance", "")
    caller_name = profile["name"]
    prompt = dialogue_prompt_template().format(
        caller_name=caller_name,
        caller_place=caller_place,
        requested_place=requested_place,
        max_dialogue_chars=MAX_DIALOGUE_CHARS,
        context_json=context_json,
        transcript=transcript,
    )
    return f"""{prompt}

AUTHORITATIVE STORY GUIDANCE:
{call_guidance}

LATEST OPERATOR UTTERANCE:
{transcript}

The authoritative story guidance and latest operator utterance take priority over
the generic example above. Follow the story guidance for this turn exactly. If it
requires a specific sentence, say that sentence; do not answer an earlier turn
or repeat the caller's opening statement.

{NATURAL_REPLY_INSTRUCTIONS}"""


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
                    "think": os.environ.get("NN_OLLAMA_THINK", "false").lower() in {"1", "true", "yes"},
                    "format": "json",
                    "stream": False,
                    "keep_alive": 600,
                    "options": {
                         "temperature": float(os.environ.get("NN_DIALOGUE_TEMPERATURE", "0.35")),
                         "num_predict": int(os.environ.get("NN_OLLAMA_DIALOGUE_NUM_PREDICT", str(MAX_OUTPUT_TOKENS))),
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
    raise SystemExit(persistent_main())
