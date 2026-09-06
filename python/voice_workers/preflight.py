"""Validate the offline voice runtime without loading model weights."""

import json
import os
import shutil
import sys

from .common import fail


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
    require_command(os.environ.get("NN_ARECORD_BINARY", "arecord"))
    require_command(os.environ.get("NN_APLAY_BINARY", "aplay"))
    require_command(os.environ.get("NN_WHISPER_CPP", "whisper-cli"))
    require_command(os.environ.get("NN_LLAMA_CPP", "llama-cli"))
    whisper_model = os.environ.get("NN_WHISPER_MODEL", "")
    qwen_model = os.environ.get("NN_QWEN3_MODEL", "")
    tts_model = os.environ.get("NN_QWEN3_TTS_MODEL", "")
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
    require_file(os.path.join(tts_model, "speech_tokenizer", "config.json"), "Qwen3-TTS tokenizer config")
    try:
        import torch
        import qwen_tts
    except ImportError as error:
        fail(f"missing offline Qwen3-TTS Python dependency: {error}")
    try:
        voice_map = json.loads(os.environ.get("NN_QWEN3_VOICE_MAP", '{"taren":"Ryan"}'))
    except json.JSONDecodeError as error:
        fail(f"NN_QWEN3_VOICE_MAP must be valid JSON: {error}")
    if not isinstance(voice_map, dict) or not voice_map.get("taren"):
        fail("NN_QWEN3_VOICE_MAP must configure the demo Subscriber voice taren")
    supported_speakers = {
        "Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee"
    }
    if any(speaker not in supported_speakers for speaker in voice_map.values()):
        fail("NN_QWEN3_VOICE_MAP contains a speaker unsupported by Qwen3-TTS CustomVoice")
    print("offline voice worker preflight passed")
    return 0


if __name__ == "__main__":
    main()
