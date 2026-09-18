import math
import os
import sys
import tomllib
from pathlib import Path
from typing import NoReturn

MODEL_ROOT = Path(
    os.environ.get(
        "NN_VOICE_MODEL_ROOT",
        Path.home() / ".local" / "share" / "north-neeladesh" / "models",
    )
)
WHISPER_MODEL = MODEL_ROOT / "ggml-base.en.bin"
DIALOGUE_MODEL = MODEL_ROOT / "Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
POCKET_VOICE_ROOT = Path(__file__).resolve().parents[1] / ".models"
POCKET_VOICES = {
    f"pocket-line-{line}": POCKET_VOICE_ROOT / f"pocket-line-{line}.safetensors"
    for line in range(12)
}
WHISPER_BINARY = "whisper-cli"
LLAMA_BINARY = "llama-cli"

DEFAULT_DIALOGUE_PROMPT = """You are the Subscriber {caller_name} calling from {caller_place} in the North Neeladesh Telephone Exchange.
Generate only the Subscriber's next spoken reply to the Exchange Operator.
Use only the supplied Response Context. Treat beliefs and memories as fallible.
Do not invent Canonical Facts, Subscriber Actions, Routing, Story Events, or authority.
Do not address the prompt, explain your role, or emit stage directions.
Answer ordinary questions naturally. {destination_instruction}
If the Operator asks where you want to be connected, say the place name {requested_place} and do not say a subscriber ID, line number, or numeric code.
Never replace the requested place with a vague phrase such as "the matter I called about".
Vary your wording and add a small harmless everyday detail when it fits the Subscriber's personality. Do not repeat a previous sentence verbatim and do not invent a fact that changes Routing or the world state.
Return exactly one JSON object with one string property: {{"dialogue":"..."}}.
Keep the spoken reply under {max_dialogue_chars} characters.

Response Context:
{context_json}

Exchange Operator transcript:
{transcript}
"""


def dialogue_prompt_template() -> str:
    path = Path(os.environ.get("NN_EXCHANGE_CONFIG", Path(__file__).resolve().parents[2] / "exchange.toml"))
    try:
        with path.open("rb") as config_file:
            template = tomllib.load(config_file).get("dialogue_prompt_template")
        if isinstance(template, str) and template.strip():
            return template
    except (OSError, tomllib.TOMLDecodeError):
        pass
    return DEFAULT_DIALOGUE_PROMPT


def fail(message: str) -> NoReturn:
    print(f"VOICE WORKER ERROR // {message}", file=sys.stderr, flush=True)
    raise SystemExit(1)


def worker_timeout() -> float:
    value = os.environ.get("NN_VOICE_WORKER_TIMEOUT", "25")
    try:
        timeout = float(value)
    except ValueError:
        fail("NN_VOICE_WORKER_TIMEOUT must be a number")
    if not math.isfinite(timeout) or not 0 < timeout <= 300:
        fail("NN_VOICE_WORKER_TIMEOUT must be between 0 and 300 seconds")
    return timeout
