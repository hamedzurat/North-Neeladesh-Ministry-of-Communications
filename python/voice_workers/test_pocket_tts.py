import tomllib
import unittest
from pathlib import Path

from voice_workers.common import POCKET_VOICE_ROOT, pocket_voices
from voice_workers.pocket_tts import validate


class PocketTtsVoiceTests(unittest.TestCase):
    def test_exchange_voice_ids_select_their_matching_embeddings(self):
        config_path = Path(__file__).resolve().parents[2] / "exchange.toml"
        with config_path.open("rb") as config_file:
            subscribers = tomllib.load(config_file)["subscribers"]

        voices = pocket_voices(config_path)
        self.assertEqual(
            voices,
            {
                subscriber["voice_id"]: POCKET_VOICE_ROOT
                / f"{subscriber['voice_id']}.safetensors"
                for subscriber in subscribers
            },
        )
        for subscriber in subscribers:
            request = {
                "engine": "pocket-tts",
                "model": "PocketTTS",
                "sample_rate": 24_000,
                "voice_id": subscriber["voice_id"],
                "text": "Hello.",
            }
            voice_id, _ = validate(request)
            self.assertIn(voice_id, voices)


if __name__ == "__main__":
    unittest.main()
