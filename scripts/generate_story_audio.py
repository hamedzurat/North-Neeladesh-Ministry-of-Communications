#!/usr/bin/env python3
"""Generate the non-realtime recordings used by story wiretaps.

The voice IDs are read from exchange.toml so changing a subscriber profile and
rerunning this command regenerates the authored recordings without changing
the story implementation.
"""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
import tomllib
import wave
from pathlib import Path


SAMPLE_RATE = 24_000
ROOT = Path(__file__).resolve().parents[1]


def profile(config: dict, line: int) -> tuple[str, str]:
    for subscriber in config.get("subscribers", []):
        if subscriber.get("line") == line:
            return str(subscriber["voice_id"]), str(subscriber["name"])
    raise ValueError(f"subscriber line {line} is not configured")


def synthesize(voice_id: str, text: str) -> list[int]:
    from voice_workers.pocket_tts import load_model

    model, states, torch = load_model()
    if voice_id not in states:
        raise ValueError(f"PocketTTS voice is not available: {voice_id}")
    samples: list[int] = []
    for chunk in model.generate_audio_stream(states[voice_id], text):
        values = chunk.detach().to(device="cpu", dtype=torch.float32).flatten().tolist()
        samples.extend(max(-32768, min(32767, round(value * 1.5 * 32767))) for value in values)
    return samples


def write_wav(path: Path, samples: list[int]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(b"".join(struct.pack("<h", sample) for sample in samples))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, default=ROOT / "exchange.toml")
    parser.add_argument("--output", type=Path, default=ROOT / "assets" / "stories")
    args = parser.parse_args()
    sys.path.insert(0, str(ROOT / "python"))
    with args.config.open("rb") as source:
        config = tomllib.load(source)

    kashem_voice, kashem_name = profile(config, 2)
    arnab_voice, arnab_name = profile(config, 3)
    bela_voice, _ = profile(config, 4)
    farhana_voice, _ = profile(config, 7)
    tariq_voice, _ = profile(config, 8)
    kamal_voice, kamal_name = profile(config, 9)
    rehana_voice, _ = profile(config, 10)
    bela_recordings = {
        "professor_arnab.wav": [
            (kashem_voice, f"{kashem_name}: I would like to speak with {arnab_name}."),
            (arnab_voice, "Arnab: Hello."),
            (
                kashem_voice,
                "Prof. Kashem: Congratulations, you got the job. Welcome to Neel University family.",
            ),
        ],
        "belabose_wrong.wav": [
            (arnab_voice, "Arnab Bhattacharjee: Hello, I am looking for Bela Bose."),
            (bela_voice, "I am Bela Bose, but I do not know Arnab Bhattacharjee."),
        ],
    }
    for filename, segments in bela_recordings.items():
        output = args.output / "bela_bose" / filename
        samples = [sample for voice_id, text in segments for sample in synthesize(voice_id, text)]
        write_wav(output, samples)
        print(f"wrote {output}")

    success = args.output / "bela_bose" / "belabose_success.m4a"
    if success.exists():
        duration = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", str(success)],
            text=True,
        ).strip()
        print(f"using supplied {success} ({duration}s)")
    else:
        print(f"place the supplied correct recording at {success}")

    dirty_recordings = {
        "kamal_farhana.wav": [
            (kamal_voice, f"{kamal_name}: I would like to place a short obituary notice for a former colleague."),
            (farhana_voice, "Farhana: Of course. Please give me the name and the details you would like printed."),
            (kamal_voice, f"{kamal_name}: Thank you. It is an ordinary notice; I appreciate your help."),
        ],
        "tariq_farhana.wav": [
            (tariq_voice, "Tariq: I need to report rotten grain at Koyal Market Depot."),
            (tariq_voice, "Tariq: The merchants are hiding what happened, and I have signed delivery slips."),
            (tariq_voice, "Tariq: Please protect my name when you investigate the report."),
        ],
        "rehana_farhana.wav": [
            (rehana_voice, "Rehana: I am calling because my newspaper insert was missing this morning."),
            (farhana_voice, "Farhana: I am sorry about that. We will make sure the next copy includes it."),
            (rehana_voice, "Rehana: Thank you. That is all I needed to know."),
        ],
    }
    for filename, segments in dirty_recordings.items():
        output = args.output / "dirty_work" / filename
        samples = [sample for voice_id, text in segments for sample in synthesize(voice_id, text)]
        write_wav(output, samples)
        print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
