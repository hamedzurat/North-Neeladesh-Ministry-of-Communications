"""Validate the offline voice runtime without loading model weights."""

import os
import shutil
import subprocess
from importlib import import_module

from .common import (
    POCKET_VOICES,
    WHISPER_BINARY,
    WHISPER_MODEL,
    fail,
)


def require_command(name: str) -> None:
    if shutil.which(name) is None:
        fail(f"missing local dependency: {name}")


def require_file(path: str, description: str) -> None:
    if not os.path.isfile(path):
        fail(f"missing local {description}: {path}")


def require_directory(path: str, description: str) -> None:
    if not os.path.isdir(path):
        fail(f"missing local {description}: {path}")


def require_path_label(path: str, label: str, description: str) -> None:
    if label not in path:
        fail(f"{description} must contain {label}: {path}")


def main() -> int:
    require_command(os.environ.get("NN_WHISPER_CPP", str(WHISPER_BINARY)))
    require_command("ollama")
    whisper_model = os.environ.get("NN_WHISPER_MODEL", str(WHISPER_MODEL))
    require_file(whisper_model, "whisper.cpp base.en model")
    require_path_label(whisper_model, "base.en", "whisper model path")
    ollama_model = os.environ.get("NN_OLLAMA_MODEL", "qwen3.5:4b")
    try:
        subprocess.run(["ollama", "show", ollama_model], check=True, capture_output=True, text=True)
    except subprocess.CalledProcessError as error:
        fail(f"Ollama model is unavailable: {ollama_model}: {error.stderr.strip()}")
    for voice_path in POCKET_VOICES.values():
        require_file(str(voice_path), "PocketTTS voice file")
    try:
        import_module("torch")
        import_module("pocket_tts")
    except Exception as error:  # noqa: BLE001 - dependency imports have backend-specific failures
        fail(f"offline voice dependency check failed: {error}")
    print("offline voice worker preflight passed")
    return 0


if __name__ == "__main__":
    main()
