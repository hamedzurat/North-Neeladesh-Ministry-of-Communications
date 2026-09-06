"""Validate the offline voice runtime without loading model weights."""

import os
import shutil
import sys

from .common import DIALOGUE_MODEL, LLAMA_BINARY, TTS_MODEL, WHISPER_BINARY, WHISPER_MODEL, fail


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
    require_command(os.environ.get("NN_LLAMA_CPP", str(LLAMA_BINARY)))
    whisper_model = os.environ.get("NN_WHISPER_MODEL", str(WHISPER_MODEL))
    qwen_model = os.environ.get("NN_QWEN3_MODEL", str(DIALOGUE_MODEL))
    tts_model = os.environ.get("NN_QWEN3_TTS_MODEL", str(TTS_MODEL))
    require_file(whisper_model, "whisper.cpp base.en model")
    require_path_label(whisper_model, "base.en", "whisper model path")
    require_file(qwen_model, "Qwen3-4B-Instruct-2507 Q4_K_M model")
    require_path_label(qwen_model, "Qwen3-4B-Instruct-2507", "dialogue model path")
    require_path_label(qwen_model, "Q4_K_M", "dialogue model path")
    require_directory(tts_model, "Qwen3-TTS 1.7B model")
    require_path_label(tts_model, "Qwen3-TTS", "TTS model path")
    require_path_label(tts_model, "1.7B", "TTS model path")
    require_path_label(tts_model, "CustomVoice", "TTS model path")
    require_file(os.path.join(tts_model, "config.json"), "Qwen3-TTS config")
    require_file(os.path.join(tts_model, "generation_config.json"), "Qwen3-TTS generation config")
    require_file(
        os.path.join(tts_model, "speech_tokenizer", "config.json"), "Qwen3-TTS tokenizer config"
    )
    try:
        import torch
        import qwen_tts
    except ImportError as error:
        fail(f"missing offline Qwen3-TTS Python dependency: {error}")
    print("offline voice worker preflight passed")
    return 0


if __name__ == "__main__":
    main()
