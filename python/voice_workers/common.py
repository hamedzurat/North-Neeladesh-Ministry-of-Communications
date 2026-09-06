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
WHISPER_BINARY = "whisper-cli"
LLAMA_BINARY = "llama-cli"


def fail(message: str) -> NoReturn:
    print(message, file=sys.stderr)
    raise SystemExit(1)
