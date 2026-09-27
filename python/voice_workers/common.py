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
POCKET_VOICE_NAMES = (
    "anna",
    "alba",
    "charles",
    "vera",
    "fantine",
    "paul",
    "eponine",
    "azelma",
    "george",
    "mary",
    "jane",
    "michael",
)
POCKET_VOICES = {
    f"pocket-line-{line}": POCKET_VOICE_ROOT / f"{name}.safetensors"
    for line, name in enumerate(POCKET_VOICE_NAMES)
}
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
    path = Path(os.environ.get("NN_EXCHANGE_CONFIG", Path(__file__).resolve().parents[2] / "exchange.toml"))
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
