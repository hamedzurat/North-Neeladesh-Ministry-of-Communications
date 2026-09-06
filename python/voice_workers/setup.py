"""Download model assets for the offline voice workers."""

import shutil

from huggingface_hub import hf_hub_download, snapshot_download

from .common import (
    DIALOGUE_MODEL,
    LLAMA_BINARY,
    MODEL_ROOT,
    TTS_MODEL,
    WHISPER_BINARY,
    WHISPER_MODEL,
)


WHISPER_REPOSITORY = "ggerganov/whisper.cpp"
DIALOGUE_REPOSITORY = "unsloth/Qwen3-4B-Instruct-2507-GGUF"
TTS_REPOSITORY = "Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice"
TTS_TOKENIZER_REPOSITORY = "Qwen/Qwen3-TTS-Tokenizer-12Hz"


def main() -> int:
    for binary in (WHISPER_BINARY, LLAMA_BINARY):
        if shutil.which(binary) is None:
            raise RuntimeError(
                f"missing {binary}; install it with: sudo pacman -S llama-cpp ggml-cuda whisper-cpp"
            )
    MODEL_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"Downloading voice models into {MODEL_ROOT}")

    hf_hub_download(WHISPER_REPOSITORY, filename=WHISPER_MODEL.name, local_dir=MODEL_ROOT)
    hf_hub_download(DIALOGUE_REPOSITORY, filename=DIALOGUE_MODEL.name, local_dir=MODEL_ROOT)
    snapshot_download(TTS_REPOSITORY, local_dir=TTS_MODEL)
    snapshot_download(TTS_TOKENIZER_REPOSITORY, local_dir=TTS_MODEL / "speech_tokenizer")

    print("Voice model setup complete")
    print(f"  STT: {WHISPER_MODEL}")
    print(f"  Dialogue: {DIALOGUE_MODEL}")
    print(f"  TTS: {TTS_MODEL}")
    print(f"  Whisper runtime: {shutil.which(WHISPER_BINARY)}")
    print(f"  Dialogue runtime: {shutil.which(LLAMA_BINARY)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
