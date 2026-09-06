"""Synthesize Subscriber speech with the local Qwen3-TTS 1.7B CustomVoice model."""

import json
import os
import struct
import sys

from .common import fail


SAMPLE_RATE = 24_000


def main() -> int:
    try:
        request = json.load(sys.stdin)
    except json.JSONDecodeError as error:
        fail(f"invalid TTS request: {error}")
    if not isinstance(request, dict):
        fail("TTS request must be a JSON object")
    if request.get("engine") != "qwen3-tts" or request.get("model") != "Qwen3-TTS-1.7B":
        fail("the MVP TTS worker accepts Qwen3-TTS 1.7B only")
    if request.get("sample_rate") != SAMPLE_RATE:
        fail("TTS request has an unsupported sample rate")
    voice_id = request.get("voice_id")
    text = request.get("text")
    if not isinstance(voice_id, str) or not voice_id or not isinstance(text, str) or not text.strip():
        fail("TTS request must contain voice_id and text")

    model_path = os.environ.get("NN_QWEN3_TTS_MODEL")
    if not model_path or not os.path.isdir(model_path):
        fail("NN_QWEN3_TTS_MODEL must point to a local Qwen3-TTS 1.7B model directory")
    try:
        voice_map = json.loads(os.environ.get("NN_QWEN3_VOICE_MAP", '{"taren":"Ryan"}'))
    except json.JSONDecodeError as error:
        fail(f"NN_QWEN3_VOICE_MAP is invalid JSON: {error}")
    speaker = voice_map.get(voice_id) if isinstance(voice_map, dict) else None
    if not isinstance(speaker, str) or not speaker:
        fail(f"no Qwen3-TTS voice is configured for Subscriber voice {voice_id!r}")

    # Keep every Hugging Face lookup offline, including the tokenizer loaded by
    # qwen-tts after the main model has been resolved.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    try:
        import torch
        from qwen_tts import Qwen3TTSModel
    except ImportError as error:
        fail(f"Qwen3-TTS Python dependencies are unavailable: {error}")

    device = os.environ.get("NN_QWEN3_TTS_DEVICE", "cuda:0")
    dtype = torch.bfloat16 if device.startswith("cuda") else torch.float32
    try:
        model = Qwen3TTSModel.from_pretrained(
            model_path,
            device_map=device,
            dtype=dtype,
            local_files_only=True,
        )
        wavs, sample_rate = model.generate_custom_voice(
            text=text,
            language="English",
            speaker=speaker,
        )
    except Exception as error:  # model runtimes expose backend-specific exception types
        fail(f"Qwen3-TTS synthesis failed: {error}")
    if sample_rate != SAMPLE_RATE or not wavs:
        fail(f"Qwen3-TTS returned unsupported sample rate {sample_rate}")

    waveform = wavs[0]
    if hasattr(waveform, "detach"):
        waveform = waveform.detach().to(device="cpu", dtype=torch.float32).flatten().tolist()
    pcm = bytearray()
    for sample in waveform:
        value = max(-1.0, min(1.0, float(sample)))
        pcm.extend(struct.pack("<h", round(value * 32767)))
    if not pcm:
        fail("Qwen3-TTS returned empty audio")
    sys.stdout.buffer.write(pcm)
    return 0


if __name__ == "__main__":
    main()
