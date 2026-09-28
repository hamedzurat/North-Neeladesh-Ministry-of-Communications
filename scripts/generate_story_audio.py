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
    nahid_voice, _ = profile(config, 11)
    bela_recordings = {
        "professor_arnab.wav": [
            (kashem_voice, f"I would like to speak with {arnab_name}."),
            (arnab_voice, f"Yes, {arnab_name} speaking."),
            (kashem_voice, "I am glad to inform you that we have liked your portfolio and you have been selected as a lecturer at Neel University. You can start from next Monday."),
            (arnab_voice, "Oh thank you so much for the opportunity. This means a lot."),
        ],
        "belabose_wrong.wav": [
            (bela_voice, "Hello"),
            (arnab_voice, "Hello, I am looking for Bela Bose."),
            (bela_voice, "Yes, this is Bela. But who am I speaking to?"),
            (arnab_voice, "I am Arnab Bhattacharjee. Sorry. But I am looking for Bela, Bela Bose."),
            (bela_voice, "Yes, this is Bela Bose."),
            (arnab_voice, "Oh. I think I have the wrong number. Sorry for the bother."),
        ],
    }
    for filename, segments in bela_recordings.items():
        output = args.output / "bela_bose" / filename
        samples = [sample for voice_id, text in segments for sample in synthesize(voice_id, text)]
        write_wav(output, samples)
        print(f"wrote {output}")

    supplied_success = args.output / "bela_bose" / "belabose_success.m4a"
    success = args.output / "bela_bose" / "belabose_success.wav"
    if supplied_success.exists():
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-v",
                "error",
                "-i",
                str(supplied_success),
                "-ar",
                str(SAMPLE_RATE),
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                str(success),
            ],
            check=True,
        )
        duration = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", str(success)],
            text=True,
        ).strip()
        print(f"converted supplied {supplied_success} to {success} ({duration}s)")
    else:
        print(f"place the supplied correct recording at {supplied_success}")

    dirty_recordings = {
        "kamal_farhana.wav": [
            (kamal_voice, f"I would like to place a short obituary notice for a former colleague."),
            (farhana_voice, "Of course. Please give me the name and the details you would like printed."),
            (kamal_voice, f"Thank you. His name was Professor Rahim. He was my colleague at Neel University."),
            (farhana_voice, "I am sorry to hear that. I will pass it on to the editorial team."),
            (kamal_voice, "Thank you."),
        ],
        "tariq_farhana.wav": [
            (tariq_voice, "I need to report about the extortion by Sumon Mia's mob at Koyal Market Depot."),
            (farhana_voice, "Ok. Are you sure this is not a misunderstanding?"),
            (tariq_voice, "Yes. This is not the first time this happened."),
            (farhana_voice, "Ok. I will send some journalists to investigate. He will get in touch with you."),
            (tariq_voice, "Ok. Thanks."),
        ],
        "rehana_farhana.wav": [
            (rehana_voice, "Hello. I would like to talk about an article idea on money plants."),
            (farhana_voice, "Yes, go on."),
            (rehana_voice, "Ok. As you know money plants are very popular as a domestic plant. So I want to publish an article on the history of money plants and its domestication."),
            (farhana_voice, "Interesting idea. I will think about that. Call me again after a day or two."),
            (rehana_voice, "Ok. Bye."),
        ],
    }
    for filename, segments in dirty_recordings.items():
        output = args.output / "dirty_work" / filename
        samples = [sample for voice_id, text in segments for sample in synthesize(voice_id, text)]
        write_wav(output, samples)
        print(f"wrote {output}")

    for victim_line in (0, 1, 4, 5, 10):
        victim_voice, victim_name = profile(config, victim_line)
        recordings = [
            (nahid_voice, f"Hello. I'm Nahid from bKash. Are you {victim_name}?"),
            (victim_voice, f"Yes this is {victim_name}."),
            (nahid_voice, "There is an urgent problem with your account. Confirm the code you received by mail this weekend, and I can secure your balance immediately."),
            (victim_voice, "I don't think I received any code."),
            (nahid_voice, "Check your mails. There should be purple envelope with a code inside."),
            (victim_voice, "Oh yes. I have found the code. It's 4201"),
            (nahid_voice, "Congratulations. Your balanced has been secured."),
            (victim_voice, "Thank you so much."),
        ]
        output = args.output / "nahid" / f"nahid_{victim_line}.wav"
        samples = [sample for voice_id, text in recordings for sample in synthesize(voice_id, text)]
        write_wav(output, samples)
        print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
