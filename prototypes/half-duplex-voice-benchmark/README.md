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

To use a different port or private data directory:

```bash
python3 prototypes/half-duplex-voice-benchmark/server.py \
  --port 9000 --data-dir /path/to/private-benchmark-data
```
