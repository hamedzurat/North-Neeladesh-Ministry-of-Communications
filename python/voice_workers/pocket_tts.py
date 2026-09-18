"""Persistent CPU-only PocketTTS worker."""

import json
import struct
import sys

from .common import POCKET_VOICES, fail

SAMPLE_RATE = 24_000
ENGINE = "pocket-tts"
MODEL = "PocketTTS"
OUTPUT_GAIN = 1.5


def encode_pcm(waveform, torch) -> bytes:
    values = waveform.detach().to(device="cpu", dtype=torch.float32).flatten().tolist()
    pcm = bytearray()
    for sample in values:
        value = float(sample) * OUTPUT_GAIN
        magnitude = abs(value)
        if magnitude > 0.75:
            value = (1.0 if value >= 0 else -1.0) * (0.75 + (magnitude - 0.75) * 0.3)
        value = max(-1.0, min(1.0, value))
        pcm.extend(struct.pack("<h", round(value * 32767)))
    return bytes(pcm)


def validate(request: object) -> tuple[str, str]:
    if not isinstance(request, dict):
        fail("TTS request must be a JSON object")
    if request.get("engine") != ENGINE or request.get("model") != MODEL:
        fail("PocketTTS received an unsupported request")
    if request.get("sample_rate") != SAMPLE_RATE:
        fail("PocketTTS requires a 24 kHz request")
    voice_id = request.get("voice_id")
    text = request.get("text")
    if not isinstance(voice_id, str) or not voice_id:
        fail("PocketTTS request must contain voice_id")
    if not isinstance(text, str) or not text.strip():
        fail("PocketTTS request must contain text")
    return voice_id, text


def load_model():
    for voice_path in POCKET_VOICES.values():
        if not voice_path.is_file():
            fail(f"PocketTTS voice file does not exist: {voice_path}")
    try:
        import torch
        from pocket_tts import TTSModel

        model = TTSModel.load_model()
        voice_states = {
            voice_id: model.get_state_for_audio_prompt(str(voice_path))
            for voice_id, voice_path in POCKET_VOICES.items()
        }
    except Exception as error:  # noqa: BLE001 - runtime errors depend on local torch
        fail(f"PocketTTS model load failed: {error}")
    if model.device.type != "cpu":
        fail(f"PocketTTS must run on CPU, got {model.device}")
    if model.sample_rate != SAMPLE_RATE:
        fail(f"PocketTTS returned unsupported sample rate {model.sample_rate}")
    return model, voice_states, torch


def main() -> int:
    if "--persistent" not in sys.argv:
        fail("PocketTTS must run in persistent mode")
    model, voice_states, torch = load_model()
    for line in sys.stdin.buffer:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            voice_id, text = validate(request)
            if voice_id not in voice_states:
                fail(f"unsupported PocketTTS voice: {voice_id!r}")
            output = sys.stdout.buffer
            emitted = False
            for chunk in model.generate_audio_stream(voice_states[voice_id], text):
                pcm = encode_pcm(chunk, torch)
                if pcm:
                    emitted = True
                    output.write(struct.pack("<I", len(pcm)))
                    output.write(pcm)
                    output.flush()
            if not emitted:
                fail("PocketTTS returned empty audio")
            output.write(struct.pack("<I", 0))
            output.flush()
        except Exception as error:  # noqa: BLE001 - worker must report model failures
            fail(f"PocketTTS synthesis failed: {error}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
