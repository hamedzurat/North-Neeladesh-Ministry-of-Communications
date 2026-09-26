"""Download model assets for the offline voice workers."""

import shutil
import subprocess
from pathlib import Path

from huggingface_hub import hf_hub_download, list_repo_files

from .common import (
    MODEL_ROOT,
    POCKET_VOICE_ROOT,
    WHISPER_BINARY,
    WHISPER_MODEL,
)

WHISPER_REPOSITORY = "ggerganov/whisper.cpp"
OLLAMA_MODEL = "qwen3.5:4b"
POCKET_VOICE_REPOSITORY = "kyutai/pocket-tts-without-voice-cloning"
POCKET_VOICE_REVISION = "e81d79e8194ad4c7ce879c87a4258ef20cbf2487"
POCKET_VOICE_DIRECTORY = "languages/english/embeddings/"


def main() -> int:
    for binary in (WHISPER_BINARY, "ollama"):
        if shutil.which(binary) is None:
            raise RuntimeError(
                f"missing {binary}; install whisper.cpp and Ollama before running voice-setup"
            )
    MODEL_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"Downloading voice models into {MODEL_ROOT}")

    hf_hub_download(WHISPER_REPOSITORY, filename=WHISPER_MODEL.name, local_dir=MODEL_ROOT)
    subprocess.run(["ollama", "pull", OLLAMA_MODEL], check=True)
    embedding_files = [
        filename
        for filename in list_repo_files(POCKET_VOICE_REPOSITORY, revision=POCKET_VOICE_REVISION)
        if filename.startswith(POCKET_VOICE_DIRECTORY) and filename.endswith(".safetensors")
    ]
    POCKET_VOICE_ROOT.mkdir(parents=True, exist_ok=True)
    for filename in embedding_files:
        downloaded_voice = hf_hub_download(
            POCKET_VOICE_REPOSITORY,
            filename=filename,
            revision=POCKET_VOICE_REVISION,
        )
        shutil.copy2(downloaded_voice, POCKET_VOICE_ROOT / Path(filename).name)
    from pocket_tts import TTSModel

    TTSModel.load_model()

    print("Voice model setup complete")
    print(f"  STT: {WHISPER_MODEL}")
    print(f"  Dialogue: Ollama {OLLAMA_MODEL}")
    print(f"  PocketTTS embeddings: {len(embedding_files)} files in {POCKET_VOICE_ROOT}")
    print(f"  Whisper runtime: {shutil.which(WHISPER_BINARY)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
