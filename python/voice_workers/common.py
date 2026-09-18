import math
import os
import sys
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
TTS_MODEL = MODEL_ROOT / "Qwen3-TTS-12Hz-1.7B-CustomVoice"
POCKET_VOICE_ROOT = Path(__file__).resolve().parents[1] / ".models"
POCKET_VOICES = {
    f"pocket-line-{line}": POCKET_VOICE_ROOT / f"pocket-line-{line}.safetensors"
    for line in range(12)
}
WHISPER_BINARY = "whisper-cli"
LLAMA_BINARY = "llama-cli"


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
