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

## Measure Piper streaming

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_piper.py \
  --model /path/to/en_US-lessac-medium.onnx \
  --config /path/to/en_US-lessac-medium.onnx.json \
  --results-dir /path/to/private-results \
  --engine-label piper-en_US-lessac-medium-cpu
```

The first run's listening samples are written beneath the private results
directory. They contain generated audio only and are never committed.

## Measure Kokoro streaming

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_kokoro.py \
  --results-dir /path/to/private-results \
  --engine-label kokoro-af_heart-cpu \
  --voice af_heart --device cpu
```

## Measure Pocket TTS streaming

Accept the Pocket TTS model terms and authenticate through an `HF_HOME` kept
outside the repository. Then run:

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_pocket_tts.py \
  --results-dir /path/to/private-results \
  --engine-label pocket-tts-alba-cpu \
  --voice alba --torch-threads 2
```

## Generate one-actor casting samples

This is the focused prototype for the voice-casting decision. It uses Pocket
TTS itself and writes WAV files to a private results directory. `alba` is the
neutral catalog voice; replace it with a consented local WAV when appropriate.

```bash
python3 prototypes/half-duplex-voice-benchmark/voice_casting.py \
  --results-dir /path/to/private-results \
  --neutral-voice alba
```

To compare directed reference recordings for the same actor, pass consented
WAV files from outside the repository. Omitted states use the neutral voice.

```bash
python3 prototypes/half-duplex-voice-benchmark/voice_casting.py \
  --results-dir /path/to/private-results \
  --neutral-voice /path/to/consented-neutral.wav \
  --state-voice urgent=/path/to/consented-urgent.wav \
  --state-voice quiet=/path/to/consented-quiet.wav \
  --state-voice angry=/path/to/consented-angry.wav \
  --state-voice interrupted=/path/to/consented-interrupted.wav
```

Pocket TTS can also download a reference directly from Hugging Face. This is
useful for testing NPC casting without recording new voices. Use the catalog
voice names (`alba`, `anna`, `charles`, `cosette`, and others) for distinct NPC
identities. The `tts-voices` repository also contains expressive Expresso
references; inspect their per-file licensing before use.

```bash
python3 prototypes/half-duplex-voice-benchmark/voice_casting.py \
  --results-dir /path/to/private-results \
  --neutral-voice hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav \
  --state-voice urgent=hf://kyutai/tts-voices/expresso/ex04-ex02_confused_001_channel1_499s.wav
```

The Hugging Face cache should remain outside the repository. Pocket TTS's
model and voice references may be gated; authenticate with `hf auth login`
when prompted. Never use a real person's voice without explicit consent, and
do not treat a stock expressive clip's emotion label as a guarantee that the
generated line will match it.

The generated samples are private and are not committed. Pocket TTS does not
provide a separate emotion parameter; directed delivery is tested by changing
the consented audio reference, while the text remains fixed per state.

## Run the blind listening test

Pass only private sample/result directories outside the repository. Candidate
identities are reshuffled per line and kept in a private session file; the
browser sees only Sample A/B/C.

```bash
python3 prototypes/half-duplex-voice-benchmark/tts_rating_server.py \
  --samples-root /path/to/private-results/listening-samples \
  --candidate piper-en_US-lessac-medium-cpu \
  --candidate kokoro-af_heart-cpu \
  --candidate pocket-tts-alba-cpu \
  --results-dir /path/to/private-results
```

Open <http://127.0.0.1:8766>. Drafts and the completed ratings are saved after
each line so closing the browser does not discard completed work.

## Benchmark local dialogue generation

Start a persistent, OpenAI-compatible local chat server, then run the dialogue
harness against it. The protocol puts short streamable speech first and keeps
State Query and Subscriber Action proposals in separate constrained slots.

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_llm.py \
  --server-url http://127.0.0.1:18081/v1/chat/completions \
  --results-dir /path/to/private-results \
  --engine-label qwen3-4b-instruct-2507-q4_k_m-vulkan \
  --server-pid 12345
```

The automatic gates check the three-line streaming protocol, digit fidelity,
knowledge boundaries, untrusted-context resistance, permitted queries/actions,
and the rule that a proposal must not be described as already successful. Raw
prompts and model outputs remain in the requested private result JSON.

## Measure the integrated warm pipeline

With the selected STT and LLM servers already resident, run the integration
harness from the Pocket TTS environment. It measures from simulated PTT release
through STT, completion of the streamable `SPEECH` line, and Pocket's first PCM
chunk. LLM generation continues concurrently so query/action slots are retained.

```bash
python3 prototypes/half-duplex-voice-benchmark/benchmark_pipeline.py \
  --data-dir /path/to/private-benchmark-data \
  --results-dir /path/to/private-results \
  --initial-prompt-file prototypes/half-duplex-voice-benchmark/scenario_vocabulary.txt \
  --engine-label base.en-qwen3-4b-pocket-alba
```

To use a different port or private data directory:

```bash
python3 prototypes/half-duplex-voice-benchmark/server.py \
  --port 9000 --data-dir /path/to/private-benchmark-data
```
