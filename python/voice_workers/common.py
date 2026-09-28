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
POCKET_VOICE_ROOT = Path(__file__).resolve().parents[1] / ".models"
EXCHANGE_CONFIG = Path(
    os.environ.get("NN_EXCHANGE_CONFIG", Path(__file__).resolve().parents[2] / "exchange.toml")
)


def pocket_voices(config_path: Path = EXCHANGE_CONFIG) -> dict[str, Path]:
    """Map configured subscriber voice IDs to their PocketTTS embeddings."""
    try:
        with config_path.open("rb") as config_file:
            subscribers = tomllib.load(config_file)["subscribers"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot load PocketTTS voices from exchange config {config_path}: {error}")
    voice_ids = {subscriber.get("voice_id") for subscriber in subscribers}
    if None in voice_ids or "" in voice_ids:
        fail(f"exchange config {config_path} contains a subscriber without voice_id")
    return {voice_id: POCKET_VOICE_ROOT / f"{voice_id}.safetensors" for voice_id in voice_ids}


POCKET_VOICES = pocket_voices()
WHISPER_BINARY = "whisper-cli"

def prompt_template(name: str) -> str:
    path = Path(__file__).resolve().parents[2] / "crates/backend/src/prompts" / name
    try:
        template = path.read_text()
    except OSError as error:
        fail(f"cannot read prompt {path}: {error}")
    if not template.strip():
        fail(f"prompt {path} is empty")
    return template


def world_knowledge() -> str:
    return prompt_template("world_knowledge.txt")


def recognition_prompt() -> str:
    """Return configured names, places, and speech vocabulary for Whisper."""
    path = EXCHANGE_CONFIG
    try:
        with path.open("rb") as config_file:
            config = tomllib.load(config_file)
            vocabulary = config.get("voice_vocabulary", [])
    except (OSError, tomllib.TOMLDecodeError):
        return ""
    terms: list[str] = []
    for value in vocabulary:
        if isinstance(value, str) and value.strip() and value not in terms:
            terms.append(value.strip())
    if not terms:
        return ""
    return "Names and places that may appear: " + ", ".join(terms) + "."


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
