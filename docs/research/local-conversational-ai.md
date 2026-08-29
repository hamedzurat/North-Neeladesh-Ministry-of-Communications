# Fully local conversational AI on the target laptop

## Decision summary

The first implementation should use three replaceable local workers, not one end-to-end speech model:

1. **STT:** benchmark `whisper.cpp` with `small.en` against `faster-whisper` with `distil-large-v3`.
2. **Dialogue:** start with `llama.cpp` serving a 4B-class, four- or five-bit model. The primary model candidate is **Qwen3-4B-Instruct-2507**; compare **Phi-4-mini-instruct** as a second model and **Qwen3.5-4B** only as an experimental candidate.
3. **TTS:** benchmark **Pocket TTS** as the current CPU-streaming candidate, keep **Piper** as the dependable low-resource baseline, and compare **Kokoro-82M** for character-voice quality.
4. **Turn detection:** PTT is authoritative for the MVP. Run **Silero VAD** in parallel to collect data and support a later automatic-turn/barge-in experiment. Treat acoustic echo cancellation as a separate prerequisite for speaker-based barge-in.

This is a shortlist, not a claim that any combination meets the aspirational 2.5-second release-to-first-audio target. The cited project benchmarks were run on other hardware and do not transfer to the RTX 5060 Laptop GPU. Select the production combination only after the whole pipeline is measured concurrently on the actual laptop.

The default experiment should keep the 4B LLM resident on the GPU, run Pocket TTS, Piper, or Kokoro on the CPU, and test both CPU and GPU STT. That allocation gives the latency-critical dialogue generator first claim on the 8,151 MiB VRAM while leaving room to discover whether GPU STT improves end-to-end latency without causing eviction, allocation failures, or thermal throttling.

## Constraints and implications

The target is Linux on one laptop with an RTX 5060 Laptop GPU (8,151 MiB reported VRAM), about 16 GiB system RAM, and an AMD Ryzen 7 260. The product needs English-only, fully offline STT → LLM → TTS, a dependable half-duplex PTT path, streaming output and cancellation, and an architectural route to barge-in. The response-time goal is the first playable audio as soon as possible, with 2.5 seconds as an aspiration rather than an acceptance criterion selected without measurements.

Three consequences follow:

- A model's advertised context length is not a reason to fill it. Long prompts add prefill latency and KV-cache memory, exactly the resources needed for fast speech response.
- Model weight size alone does not establish fit. Runtime buffers, KV cache, CUDA context, audio models, the GUI, and driver reservations all count. Four-bit weights for a 4B dense model have a theoretical payload near 2 GB before quantization metadata and runtime state; actual resident memory must be measured.
- The game needs process-level provider boundaries even if some workers later become in-process libraries. A stable typed contract makes the Odin/Rust game core independent of Python, CUDA, model format, and voice-engine churn.

## Candidate comparison

### Speech to text

| Candidate | What primary sources establish | Fit and risk on this project |
| --- | --- | --- |
| `whisper.cpp` + `small.en` | MIT-licensed C/C++; Linux, CPU, NVIDIA CUDA, a C API, integer quantization, VAD, and a real-time microphone example are supported. Its own memory table reports about 852 MB for `small`, 2.1 GB for `medium`, and 3.9 GB for `large`; these figures are project-level reference values, not measurements on this laptop. ([repository and memory table](https://github.com/ggml-org/whisper.cpp#memory-usage), [real-time example](https://github.com/ggml-org/whisper.cpp#real-time-audio-input-example), [VAD documentation](https://github.com/ggml-org/whisper.cpp#voice-activity-detection-vad), [license](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE)) | Best low-dependency baseline and easiest compiled integration. `small.en` is the first model to test because English-only is sufficient and its documented memory is modest. The included streaming example is explicitly described as naive, so PTT should submit a completed utterance rather than trusting partial hypotheses for game decisions. |
| `faster-whisper` + `distil-large-v3` | `faster-whisper` is MIT licensed, supports CUDA FP16 and INT8, integrates Silero VAD, and explicitly supports `distil-large-v3`. Its current CUDA path requires CUDA 12 cuBLAS and cuDNN 9. The project's GPU memory numbers are from an RTX 3070 Ti 8 GB and therefore are comparison evidence only. ([repository, requirements, and benchmark](https://github.com/SYSTRAN/faster-whisper#benchmark), [Distil-Whisper integration](https://github.com/SYSTRAN/faster-whisper#faster-distil-whisper), [license](https://github.com/SYSTRAN/faster-whisper/blob/master/LICENSE)) `distil-large-v3` is a 756M-parameter English model under MIT; its authors report 6.3× the relative speed of large-v3 and within 1% WER for their long-form evaluation. ([official model card](https://huggingface.co/distil-whisper/distil-large-v3)) | Strong quality candidate, but Python/CTranslate2/cuDNN creates more packaging risk and it may compete with the LLM for VRAM. Test GPU INT8/FP16 and CPU INT8, one utterance at a time; batching is irrelevant to this game and raises memory. Do not adopt its published accuracy or speed numbers as target-machine results. |
| `whisper.cpp` + `base.en` | The same runtime reports roughly 388 MB memory for `base`. ([memory table](https://github.com/ggml-org/whisper.cpp#memory-usage)) | Useful emergency/degraded profile and a latency floor. Keep only if it transcribes the project's proper names, four-digit numbers, accents, and noisy telephone-filtered speech reliably enough. |

The initial STT matrix is therefore `small.en` on CPU and CUDA, `base.en` as the low-resource fallback, and `distil-large-v3` through faster-whisper on CPU INT8 and GPU INT8/FP16. Large Whisper variants should not enter the first matrix: the target is short English PTT turns, and their memory would reduce the LLM's co-residency headroom before any target-specific evidence justifies it.

### Dialogue model and inference runtime

Use `llama.cpp` as the first inference runtime. It is MIT-licensed C/C++, supports CPU/CUDA execution and quantized GGUF models, and its server provides streaming token output, schema-constrained JSON, prompt-cache reuse, monitoring, and OpenAI-compatible endpoints. ([repository](https://github.com/ggml-org/llama.cpp), [server features and API](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md), [license](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE)) These features match a compiled game core without requiring the core to link model code directly.

| Model | What primary sources establish | Fit and risk |
| --- | --- | --- |
| **Qwen3-4B-Instruct-2507** | A 4B non-thinking instruction model under Apache 2.0. Qwen describes the Instruct-2507 release as improving instruction following, tool usage, and open-ended text generation. ([model card and license](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507), [Qwen release documentation](https://github.com/QwenLM/Qwen3#qwen3-2507)) | Primary candidate. Non-thinking output avoids spending the voice latency budget on hidden deliberation. Quantize the official checkpoint into GGUF at pinned revisions rather than silently depending on an unreviewed community quant. Test Q4_K_M and Q5_K_M; quality and actual memory decide between them. |
| **Phi-4-mini-instruct** | A 3.8B model under MIT with 128K context; Microsoft specifically identifies memory/compute-constrained and latency-bound scenarios and says post-training includes instruction following and function calling. ([official model card](https://huggingface.co/microsoft/Phi-4-mini-instruct)) | Main alternative. Its dialogue character, fact adherence, and structured-action reliability must be evaluated on the game's own scripts; general reasoning benchmark results do not answer those questions. |
| **Qwen3.5-4B** | A 4B Apache-2.0 hybrid vision-language model. Its official card says it thinks by default and documents a non-thinking API setting. ([official model card](https://huggingface.co/Qwen/Qwen3.5-4B)) `llama.cpp` has Qwen3.5 support, but open upstream reports document tokenizer, conversion, thinking-mode, and structured-tool-call defects in some builds/configurations. ([tokenizer report](https://github.com/ggml-org/llama.cpp/issues/21919), [conversion report](https://github.com/ggml-org/llama.cpp/issues/23758), [structured tool-call report](https://github.com/ggml-org/llama.cpp/issues/21158)) | Experimental only for the first decision. It may prove stronger or faster, but its newer hybrid architecture and current integration churn are extra demo risk. Promote it only if a pinned build passes the exact structured-output and long-session suite. |

Do not begin with an 8B model merely because four-bit weights appear to fit. Co-resident audio models, context cache, and thermal behavior matter, and a responsive 4B model with tightly selected context is more valuable for live dialogue than a stronger model that misses the audio deadline. Add one 8B Q4 candidate only after a 4B baseline is working, as a quality ceiling and evidence for whether partial CPU offload is worthwhile.

The LLM must produce a single schema-constrained turn result containing dialogue plus proposed actions. `llama.cpp` supports a JSON Schema parameter and streaming, but schema validity is only the first gate: the game core must still reject unknown actions, impossible transitions, invented IDs, and stale call/session IDs. ([schema and stream API](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md#post-completion-given-a-prompt-it-returns-the-predicted-completion))

### Text to speech

| Candidate | What primary sources establish | Fit and risk |
| --- | --- | --- |
| **Pocket TTS** | Kyutai's runtime is MIT licensed; the gated model weights are CC-BY-4.0 with explicit prohibited-use terms, and the voice catalog has per-voice licenses. It is a 100M-parameter CPU model with Python/CLI APIs, audio streaming, and voice cloning. Its maintainers report about 200 ms to the first audio chunk and about 6× real time using two CPU cores on a MacBook Air M4; those numbers are not target-laptop results. ([official repository and code license](https://github.com/kyutai-labs/pocket-tts), [official model card, access terms, and model license](https://huggingface.co/kyutai/pocket-tts)) | First streaming benchmark because it preserves GPU capacity for the LLM/STT and directly exposes streamed chunks. It is new, uses Python and PyTorch 2.5+, and currently lacks an authored silence/pause feature, so measure stability, CPU contention, pronunciation, expressiveness, and cancellation rather than adopting its M4 result. Audit each bundled/cloned voice, preserve required attribution, and obtain consent for any cloning. |
| **Piper** | The current Open Home Foundation engine is a local neural TTS with Python and C/C++ APIs, raw output suitable for streaming, and a GPL-3.0 code license. Its docs warn that each voice has separate licensing information in its `MODEL_CARD`; some voices can be restrictive. The common `en_US-lessac-medium` model file is about 63 MB. ([engine](https://github.com/OHF-Voice/piper1-gpl), [CLI/raw output](https://github.com/OHF-Voice/piper1-gpl/blob/main/docs/CLI.md), [C/C++ API](https://github.com/OHF-Voice/piper1-gpl/blob/main/libpiper/README.md), [voice-license warning](https://github.com/OHF-Voice/piper1-gpl/blob/main/docs/VOICES.md), [voice manifest](https://huggingface.co/rhasspy/piper-voices/blob/main/voices.json)) | Dependable CPU baseline, small enough to keep several character voices available, and the C API reduces worker complexity. Keep a synthesizer resident; the CLI docs say repeated process/model loading is slow. GPL-3.0 implications must be reviewed if distributing linked binaries, and every selected voice needs a license audit. |
| **Kokoro-82M** | An 82M-parameter, Apache-2.0 model. The official model repository is about 363 MB and its API yields audio chunks from a generator at 24 kHz. ([official model card](https://huggingface.co/hexgrad/Kokoro-82M)) | Best quality-oriented experiment with many voices from one small model. Python/PyTorch and grapheme-to-phoneme dependencies add packaging and warm-up risk. Chunk yielding is promising but does not establish target TTFA; measure sentence segmentation, first chunk time, pronunciation, and cancellation. |

Piper should ship as the fallback even if Pocket TTS or Kokoro wins on voice quality and first-chunk latency. Chunk dialogue into clauses or sentences and synthesize as soon as a complete safe boundary arrives. Never feed incomplete JSON or unvalidated action text to TTS. Stop playback immediately on cancellation; a worker that cannot cancel inference internally may finish and discard its obsolete result in the background.

### VAD, streaming, and later barge-in

Silero VAD is the first VAD candidate. It is MIT-licensed, has PyTorch and ONNX paths, supports 8 and 16 kHz audio, and its maintainers describe a roughly 2 MB model processing 30+ ms chunks in under 1 ms on one CPU thread under their test conditions. ([official repository](https://github.com/snakers4/silero-vad), [license](https://github.com/snakers4/silero-vad/blob/master/LICENSE)) `whisper.cpp` and `faster-whisper` both already document Silero integration, which limits duplicate experimentation.

For the MVP, PTT release defines end-of-turn; VAD may trim leading/trailing silence but must not override the button. Later automatic turn detection can use VAD with minimum-speech, hangover, and maximum-utterance thresholds tuned on actual cabinet noise.

Barge-in has two distinct levels:

1. **PTT barge-in:** pressing PTT stops the audio output queue and cancels/invalidates the current LLM/TTS generation. This does not require the microphone to recognize speech while the speaker is active and should be the first improvement.
2. **Hands-free acoustic barge-in:** the microphone must distinguish the player from the game's own speaker. This requires a synchronized copy of the render stream and an acoustic echo canceller before VAD/STT. WebRTC's maintained audio-processing tree includes AEC, noise suppression, gain control, and VAD modules; SpeexDSP exposes a smaller BSD-licensed C acoustic-echo-cancellation API that consumes recorded and playback-reference frames. ([WebRTC audio-processing source](https://webrtc.googlesource.com/src/+/refs/heads/main/modules/audio_processing/), [SpeexDSP AEC API](https://github.com/xiph/speexdsp/blob/master/include/speex/speex_echo.h))

Speaker output through an ESP32 or separate USB path raises integration risk because the laptop still needs a time-aligned playback reference for AEC. Preserve the exact PCM stream and timestamps sent to the hardware, define buffering latency in the frontend protocol, and make hands-free barge-in conditional on measured echo suppression. A headset is a useful diagnostic control but not proof that the cabinet speaker path works.

## Bounded context and NPC memory

Do not append the full transcript forever. Keep the full raw transcript and inference trace for audit/replay, but construct each inference prompt from a bounded, typed **turn packet**:

```text
immutable contract
  output schema, global safety/authority rules, dialogue style constraints
current canonical slice
  NPC identity/voice, relevant world facts, active rules, call premise,
  current circuit and story state, facts this NPC is allowed to know
retrieved memory slice
  a small set of structured memories selected by NPC/run, participants,
  topic/fact IDs, salience, and recency
short conversational window
  the last few player/NPC turns verbatim
current input
  STT transcript plus confidence/alternatives for critical entities
```

Use separate stores for separate kinds of truth:

- **Canonical TOML:** authored people, relationships, world facts, rules, premises, event definitions, and allowed actions. The model cannot modify it.
- **Authoritative run state:** accepted events, relationship/suspicion factors, facts revealed, and world changes. Only the game core writes it.
- **NPC memory records:** compact facts such as `operator_refused_call`, with subject/object IDs, source event, shift, confidence, secrecy, salience, and expiry. Cross-NPC sharing occurs only through an accepted authored action/event.
- **Conversation summary:** a bounded convenience record for tone and unresolved threads. It is never allowed to override canonical or event state.
- **Audit log:** raw audio reference, STT result and alternatives, exact assembled prompt/model/config, streamed model output, validator decision, TTS input, timings, and cancellation reason. It is not automatically sent back to the model.

At the end of each accepted turn, deterministic code records game events and applies valid actions. A summarizer may propose memory records, but the core accepts only records grounded in the transcript or accepted events and never converts a generated statement into a canonical fact. Before the next turn, retrieval selects only relevant records under a configured token budget. Start with a 4–8K total prompt budget and measure quality before increasing it, despite the much larger maximum contexts advertised by Qwen and Phi.

Keep the immutable prefix byte-for-byte stable where possible. `llama.cpp` documents prompt-cache reuse for the common prefix and persistent slot cache endpoints, but it also warns that cache reuse can change bit-for-bit results on some backends, so benchmark it and do not confuse a performance cache with game memory. ([prompt-cache behavior](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md#post-completion-given-a-prompt-it-returns-the-predicted-completion), [slot save/restore](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md#post-slotsid_slotactionsave-save-the-prompt-cache-of-the-specified-slot-to-a-file))

## Worker contract and cancellation

Use versioned local IPC (Unix-domain socket or loopback HTTP during prototyping) with generated/validated schemas. The game core owns IDs and deadlines.

- `BeginTranscription(turn_id, pcm_format, audio_stream)` → partial diagnostics and one final transcript.
- `GenerateNpcTurn(turn_id, npc_id, state_revision, turn_packet, deadline)` → streamed safe dialogue units plus one final structured result.
- `Synthesize(utterance_id, voice_id, text, deadline)` → PCM chunks with sample rate and sequence numbers.
- `Cancel(id, reason)` applies to every request.

Every result echoes `turn_id` and `state_revision`. The core discards late or stale results. TTS receives only dialogue text that has passed parsing and validation. For lowest perceived latency, the dialogue protocol may stream complete clauses before the entire turn is finished, but only if actions are emitted separately at the end and later tokens cannot revise already-spoken facts. Otherwise wait for the short JSON result and synthesize immediately.

Keep provider adapters behind these contracts:

```text
Odin GUI / ESP32 frontend -> authoritative game core
                                      |
                        local AI orchestrator
                         /       |       \
                     STT       LLM       TTS
```

The process split is not a mandate for Docker. Pin native worker environments, model hashes, CUDA/runtime versions, and launch them under one supervisor. A health check and warm-up pass should complete before a Shift can begin.

## Concrete benchmark protocol

### 1. Freeze the test environment

Record OS/kernel, NVIDIA driver, CUDA and cuDNN versions, power mode, laptop plugged/unplugged state, room temperature, audio devices and rates, git commits/package lockfiles, exact model hashes, quantization, context size, GPU-layer count, thread counts, and sampling parameters. Disable unrelated GPU applications. Run one cold-start and at least five warm runs; report medians and p95, not the best run.

### 2. Build a project-specific speech set

Record at least 100 short English utterances through the actual microphone and intended cabinet geometry. Include:

- all NPC and place names;
- every four-digit subscriber number, including confusable pairs;
- connect/refuse/report phrases and corrections;
- quiet, fan noise, simulated line noise, and TTS playing from the intended speaker;
- the primary operator plus several accents/speaking rates if other people may demo it.

Keep exact human transcripts. Measure normalized WER, exact digit-string accuracy, exact proper-name accuracy, false speech triggers, clipped first/last words, PTT-release-to-final-transcript p50/p95/max, real-time factor, peak RSS, peak VRAM, and GPU/CPU utilization. Score critical entities separately; a low average WER can hide unusable number recognition.

### 3. Evaluate dialogue on game-shaped turns

Create at least 100 deterministic turn packets covering ordinary calls, lies, ambiguity, restricted knowledge, contradictory player claims, repeated questions, interruptions, stale responses, allowed and forbidden actions, and long-session memory retrieval. For each LLM/quantization/context setting measure:

- JSON/schema parse rate;
- valid action rate and forbidden/invented action rate;
- canonical-fact contradiction and secret-leak counts;
- requested-callee/four-digit fact accuracy;
- character consistency and naturalness by blinded human A/B rating;
- prompt tokens, prompt-evaluation time, first-token latency, output tokens/second, full-turn time, peak RSS/VRAM;
- response length and whether dialogue reaches a TTS-safe clause quickly.

Run multiple seeds/temperatures for generative quality, but maintain a deterministic low-temperature suite for regression. A model fails regardless of prose quality if it emits an invalid action, leaks private facts, or cannot stay inside the latency/output-length budget.

### 4. Evaluate TTS independently

Use 50–100 lines containing names, numbers, abbreviations, emotional directions, questions, interruptions, and deliberately short replies. Measure warm and cold text-to-first-PCM, text-to-first-playable-audio, total synthesis time/real-time factor, peak RSS/VRAM, pronunciation error count, and audio artifacts at chunk boundaries. Conduct blinded listening for intelligibility, naturalness, character fit, and fatigue. Audit the engine and every chosen voice/model license in the results table.

For cancellation, stop at random offsets in 100 utterances. Measure press-to-silence p95, stale chunks played after cancellation, worker recovery, and whether the next utterance starts cleanly. The target should be perceptually immediate (define a numeric threshold only after the audio backend's buffer size is known).

### 5. Measure the pipeline, not just its parts

Test at least these co-residency profiles:

| Profile | STT | LLM | TTS | Purpose |
| --- | --- | --- | --- | --- |
| A | `whisper.cpp small.en`, CPU | Qwen3 4B Q4, CUDA | Piper, CPU | Low-risk baseline |
| B | `whisper.cpp small.en`, CUDA | Qwen3 4B Q4, CUDA | Pocket TTS, CPU | CPU streaming TTS plus GPU STT |
| C | faster-whisper Distil-Large-v3 INT8, CPU | Qwen3 4B Q4, CUDA | Pocket TTS, CPU | Quality STT with no audio model on GPU |
| D | winning STT | Qwen3 4B Q4, CUDA | Kokoro, CPU | Character-voice alternative |
| E | winning STT | Phi-4 mini Q4, CUDA | winning TTS | LLM quality/latency alternative |

For 30 consecutive representative turns, timestamp microphone start, PTT release, final transcript, LLM request, first token, first complete speakable unit, first TTS PCM, first audio submitted, first audible output estimate, final result, and cancellation completion. Sample VRAM/RSS/utilization/temperature continuously. Run the simple game loop and ESP32/GUI message traffic simultaneously.

Primary outcome is release-to-first-playable-audio p50/p95/max. Also report quality gates, OOM/retry count, missed deadlines, and latency after thermal steady state. The 2.5-second goal is achieved only if p95 stays below it in the 30-turn integrated run with acceptable speech and dialogue quality; otherwise report the Pareto frontier rather than concealing quality loss.

### 6. Barge-in experiments after the MVP

First test PTT interruption with TTS playback active: audio stops, current turn is invalidated, and a new recording begins without stale output. Then build an acoustic set with the real speaker/microphone placement and compare no AEC, SpeexDSP, and WebRTC APM. Measure echo-only false triggers, near-end speech detection while TTS plays, post-AEC STT entity accuracy, cancellation time, and varying speaker volume/distance. Hands-free mode ships only if it beats predefined false-trigger and missed-barge-in thresholds across this set.

## Recommended adoption order

1. Implement the worker contracts and the instrumentation harness.
2. Establish Profile A with `whisper.cpp small.en`, Qwen3-4B-Instruct-2507 Q4 through `llama.cpp`, and Piper.
3. Add Pocket TTS first, then Phi-4-mini, faster-whisper/Distil-Whisper, and Kokoro one dimension at a time.
4. Select the production stack from integrated p95 latency, critical-fact accuracy, action safety, voice quality, stability, and license fit—not from generic leaderboards.
5. Keep the losing low-resource STT/TTS pair as an offline recovery profile if it is reliable.
6. Only then test Qwen3.5, an 8B quality ceiling, PTT barge-in, and finally acoustic hands-free barge-in.

This order gives the university demo a fully local, low-dependency fallback while leaving every model provider replaceable for later PEFT, better voices, or more advanced duplex audio.
