import math
import os
import re
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

DEFAULT_DIALOGUE_PROMPT = """You are a configured subscriber in a telephone exchange.
Generate only the subscriber's next spoken reply to the Exchange Operator.
Use only the supplied Response Context. Do not invent facts, actions, routing, events, authority, or world state.
Do not address the prompt, explain your role, or emit stage directions.
Return exactly one JSON object with one string property: {{"dialogue":"..."}}.
Keep the spoken reply under {max_dialogue_chars} characters.

Response Context:
{context_json}

Exchange Operator transcript:
{transcript}

Example output:
{{"dialogue":"I will answer that."}}
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


def recognition_prompt() -> str:
    """Return configured names, places, and speech vocabulary for Whisper."""
    path = Path(os.environ.get("NN_EXCHANGE_CONFIG", Path(__file__).resolve().parents[2] / "exchange.toml"))
    try:
        with path.open("rb") as config_file:
            config = tomllib.load(config_file)
            subscribers = config.get("subscribers", [])
            vocabulary = config.get("voice_vocabulary", [])
    except (OSError, tomllib.TOMLDecodeError):
        return ""
    terms: list[str] = []
    for value in vocabulary:
        if isinstance(value, str) and value.strip() and value not in terms:
            terms.append(value.strip())
    for subscriber in subscribers:
        if not isinstance(subscriber, dict):
            continue
        for key in ("place", "name"):
            value = subscriber.get(key)
            if isinstance(value, str) and value.strip() and value not in terms:
                terms.append(value.strip())
    return ", ".join(terms)


def normalize_transcript(transcript: str) -> str:
    """Apply configured, deterministic corrections to known Whisper aliases."""
    path = Path(os.environ.get("NN_EXCHANGE_CONFIG", Path(__file__).resolve().parents[2] / "exchange.toml"))
    try:
        with path.open("rb") as config_file:
            aliases = tomllib.load(config_file).get("voice_aliases", {})
    except (OSError, tomllib.TOMLDecodeError):
        return transcript
    if not isinstance(aliases, dict):
        return transcript
    result = transcript
    for alias, canonical in sorted(aliases.items(), key=lambda item: len(str(item[0])), reverse=True):
        if not isinstance(alias, str) or not isinstance(canonical, str):
            continue
        pattern = r"(?<!\w)" + re.escape(alias) + r"(?!\w)"
        result = re.sub(pattern, canonical, result, flags=re.IGNORECASE)
    return result


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
