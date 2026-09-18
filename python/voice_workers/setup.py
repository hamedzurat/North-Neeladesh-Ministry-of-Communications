"""Download model assets for the offline voice workers."""

import shutil
import subprocess
from pathlib import Path

from huggingface_hub import hf_hub_download, snapshot_download

from .common import (
    LLAMA_BINARY,
    MODEL_ROOT,
    POCKET_VOICES,
    WHISPER_BINARY,
    WHISPER_MODEL,
)

WHISPER_REPOSITORY = "ggerganov/whisper.cpp"
OLLAMA_MODEL = "qwen3.5:4b"
POCKET_VOICE_REPOSITORY = "kyutai/pocket-tts-without-voice-cloning"
POCKET_VOICE_REVISION = "e81d79e8194ad4c7ce879c87a4258ef20cbf2487"
POCKET_VOICE_NAMES = (
    "anna",
    "alba",
    "charles",
    "vera",
    "fantine",
    "paul",
    "eponine",
    "azelma",
    "george",
    "mary",
    "jane",
    "michael",
)
POCKET_VOICE_FILES = {
    f"pocket-line-{line}": f"languages/english/embeddings/{name}.safetensors"
    for line, name in enumerate(POCKET_VOICE_NAMES)
}


def main() -> int:
    for binary in (WHISPER_BINARY, LLAMA_BINARY):
        if shutil.which(binary) is None:
            raise RuntimeError(
                f"missing {binary}; install it with: sudo pacman -S llama-cpp ggml-cuda whisper-cpp"
            )
    MODEL_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"Downloading voice models into {MODEL_ROOT}")

    hf_hub_download(WHISPER_REPOSITORY, filename=WHISPER_MODEL.name, local_dir=MODEL_ROOT)
    subprocess.run(["ollama", "pull", OLLAMA_MODEL], check=True)
    for voice_id, filename in POCKET_VOICE_FILES.items():
        downloaded_voice = hf_hub_download(
            POCKET_VOICE_REPOSITORY,
            filename=filename,
            local_dir=POCKET_VOICES[voice_id].parent,
            revision=POCKET_VOICE_REVISION,
        )
        Path(downloaded_voice).replace(POCKET_VOICES[voice_id])
    from pocket_tts import TTSModel

    TTSModel.load_model()

    print("Voice model setup complete")
    print(f"  STT: {WHISPER_MODEL}")
    print(f"  Dialogue: Ollama {OLLAMA_MODEL}")
    print(f"  PocketTTS voices: {len(POCKET_VOICES)} files in {next(iter(POCKET_VOICES.values())).parent}")
    print(f"  Whisper runtime: {shutil.which(WHISPER_BINARY)}")
    print(f"  Dialogue runtime: {shutil.which(LLAMA_BINARY)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
