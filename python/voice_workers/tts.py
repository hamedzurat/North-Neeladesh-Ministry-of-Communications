"""Synthesize Subscriber speech with the local Qwen3-TTS 1.7B CustomVoice model."""

import json
import os
import re
import struct
import sys
from contextlib import redirect_stdout

from .common import TTS_MODEL, fail


SAMPLE_RATE = 24_000
SUPPORTED_SPEAKERS = frozenset(
    {"Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee"}
)


def response_chunks(text: str) -> list[str]:
    chunks = [chunk.strip() for chunk in re.split(r"(?<=[.!?])\s+", text.strip()) if chunk.strip()]
    return chunks or [text.strip()]


def encode_waveform(waveform, torch) -> bytes:
    if hasattr(waveform, "detach"):
        waveform = waveform.detach().to(device="cpu", dtype=torch.float32).flatten().tolist()
    pcm = bytearray()
    for sample in waveform:
        value = max(-1.0, min(1.0, float(sample)))
        pcm.extend(struct.pack("<h", round(value * 32767)))
    return bytes(pcm)


def validated_request(request: object) -> tuple[str, str]:
    if not isinstance(request, dict):
        fail("TTS request must be a JSON object")
    if request.get("engine") != "qwen3-tts" or request.get("model") != "Qwen3-TTS-1.7B":
        fail("the MVP TTS worker accepts Qwen3-TTS 1.7B only")
    if request.get("sample_rate") != SAMPLE_RATE:
        fail("TTS request has an unsupported sample rate")
    voice_id = request.get("voice_id")
    text = request.get("text")
    if (
        not isinstance(voice_id, str)
        or not voice_id
        or not isinstance(text, str)
        or not text.strip()
    ):
        fail("TTS request must contain voice_id and text")

    model_path = os.environ.get("NN_QWEN3_TTS_MODEL", str(TTS_MODEL))
    if not os.path.isdir(model_path):
        fail(f"Qwen3-TTS model does not exist: {model_path}")
    if voice_id not in SUPPORTED_SPEAKERS:
        fail(f"unsupported Qwen3-TTS CustomVoice speaker: {voice_id!r}")
    return voice_id, text


def load_model():
    model_path = os.environ.get("NN_QWEN3_TTS_MODEL", str(TTS_MODEL))
    if not os.path.isdir(model_path):
        fail(f"Qwen3-TTS model does not exist: {model_path}")

    # Keep every Hugging Face lookup offline, including the tokenizer loaded by
    # qwen-tts after the main model has been resolved.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    try:
        with redirect_stdout(sys.stderr):
            import torch
            from qwen_tts import Qwen3TTSModel
    except ImportError as error:
        fail(f"Qwen3-TTS Python dependencies are unavailable: {error}")

    device = os.environ.get("NN_QWEN3_TTS_DEVICE", "cuda:0")
    dtype = torch.bfloat16 if device.startswith("cuda") else torch.float32
    attention = os.environ.get("NN_QWEN3_TTS_ATTENTION", "sdpa")
    if device.startswith("cuda"):
        torch.backends.cuda.enable_flash_sdp(True)
        torch.backends.cuda.enable_mem_efficient_sdp(True)
        torch.backends.cuda.enable_math_sdp(True)
    try:
        with redirect_stdout(sys.stderr):
            model = Qwen3TTSModel.from_pretrained(
                model_path,
                device_map=device,
                dtype=dtype,
                attn_implementation=attention,
                local_files_only=True,
            )
    except Exception as error:  # model runtimes expose backend-specific exception types
        fail(f"Qwen3-TTS model load failed: {error}")
    return model, torch


def pcm_chunks(model, torch, speaker: str, text: str):
    with redirect_stdout(sys.stderr):
        for chunk in response_chunks(text):
            wavs, sample_rate = model.generate_custom_voice(
                text=chunk,
                language="English",
                speaker=speaker,
                # Qwen currently simulates incremental text input; each sentence
                # still returns one complete waveform before it can be emitted.
                non_streaming_mode=False,
            )
            if sample_rate != SAMPLE_RATE or not wavs:
                fail(f"Qwen3-TTS returned unsupported sample rate {sample_rate}")
            pcm = encode_waveform(wavs[0], torch)
            if not pcm:
                fail("Qwen3-TTS returned empty audio")
            yield pcm


def main() -> int:
    if "--persistent" in sys.argv:
        return persistent_main()
    try:
        request = json.load(sys.stdin)
    except json.JSONDecodeError as error:
        fail(f"invalid TTS request: {error}")
    speaker, text = validated_request(request)
    model, torch = load_model()
    audio_output = sys.stdout.buffer
    try:
        for pcm in pcm_chunks(model, torch, speaker, text):
            audio_output.write(pcm)
            audio_output.flush()
    except Exception as error:  # model runtimes expose backend-specific exception types
        fail(f"Qwen3-TTS synthesis failed: {error}")
    return 0


def persistent_main() -> int:
    model, torch = load_model()
    audio_output = sys.stdout.buffer
    for line in sys.stdin.buffer:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            speaker, text = validated_request(request)
            for pcm in pcm_chunks(model, torch, speaker, text):
                audio_output.write(struct.pack("<I", len(pcm)))
                audio_output.write(pcm)
                audio_output.flush()
            audio_output.write(struct.pack("<I", 0))
            audio_output.flush()
        except Exception as error:  # model runtimes expose backend-specific exception types
            fail(f"Qwen3-TTS synthesis failed: {error}")
    return 0


if __name__ == "__main__":
    main()
