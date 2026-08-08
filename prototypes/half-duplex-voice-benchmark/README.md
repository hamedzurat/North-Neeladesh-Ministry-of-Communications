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

To use a different port or private data directory:

```bash
python3 prototypes/half-duplex-voice-benchmark/server.py \
  --port 9000 --data-dir /path/to/private-benchmark-data
```
