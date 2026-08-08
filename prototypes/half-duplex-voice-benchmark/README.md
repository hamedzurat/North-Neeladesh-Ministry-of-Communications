# Half-duplex voice benchmark prototype

> **THROWAWAY PROTOTYPE** — this harness exists only to answer the pipeline
> selection question in the Wayfinder ticket. It is not game code.

The first stage records a small, project-specific English speech corpus through
the target laptop's microphone. Recordings stay under the ignored `data/`
directory and must not be committed.

## Run the recording stage

```bash
python3 prototypes/half-duplex-voice-benchmark/server.py
```

Open <http://127.0.0.1:8765>, allow microphone access, and follow the prompts.
The server requires `ffmpeg`, binds only to localhost, and converts browser audio
to mono 16 kHz PCM WAV files for the STT benchmark.

## Run a persistent STT server benchmark

Start the selected STT server separately, then run:

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_stt.py \
  --server-url http://127.0.0.1:18080/inference \
  --data-dir /path/to/private-benchmark-data \
  --results-dir /path/to/private-results \
  --engine-label whisper.cpp-base.en-cpu \
  --initial-prompt-file prototypes/half-duplex-voice-benchmark/scenario_vocabulary.txt \
  --server-pid 12345
```

The runner uses only the Python standard library. It reports corpus WER,
project-critical phrase accuracy, warm request latency, real-time factor, and
the server's observed resident memory. Raw transcripts and detailed results are
written only to the requested private results directory.

Run each STT candidate both without a hint and with the bounded authored
Scenario vocabulary. The hint contains spellings only; it must not inject
facts, numbers, or state that could turn a recognition error into a valid
command.

The `faster-whisper` runner uses the same corpus and scoring, but must be run
from an isolated environment containing `faster-whisper`:

```bash
uv venv --python 3.12 /path/to/private-faster-whisper-env
uv pip install --python /path/to/private-faster-whisper-env/bin/python \
  -r prototypes/half-duplex-voice-benchmark/requirements-faster-whisper.txt

python3 prototypes/half-duplex-voice-benchmark/benchmark_faster_whisper.py \
  --data-dir /path/to/private-benchmark-data \
  --results-dir /path/to/private-results \
  --download-root /path/to/private-model-cache \
  --engine-label faster-whisper-distil-large-v3-cpu-int8 \
  --model distil-large-v3 --device cpu --compute-type int8
```

For CUDA, expose the isolated environment's `nvidia/cublas/lib` and
`nvidia/cudnn/lib` directories through `LD_LIBRARY_PATH`, then use
`--device cuda --compute-type float16`. Do not use the vocabulary hint with
Distil-Whisper until its observed prompt-induced repetitions and digit
corruption are understood.

To use a different port or private data directory:

```bash
python3 prototypes/half-duplex-voice-benchmark/server.py \
  --port 9000 --data-dir /path/to/private-benchmark-data
```
