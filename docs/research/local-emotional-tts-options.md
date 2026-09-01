# Local and offline emotional TTS options

Research date: 2026-09-01

This note evaluates local text-to-speech systems for North Neeladesh's target
laptop: Linux, RTX 5060 Laptop with 8 GB VRAM, Ryzen 7 260, and about 16 GB
system RAM. The goal is short NPC lines, distinct repeatable voices, expressive
delivery, and audio that can start playing before a whole line finishes.

The hardware judgements below are feasibility judgements, not benchmark
results. None of the cited vendor measurements were made on this laptop. A
candidate marked "yes" still needs a local benchmark.

## Recommendation

Start with the existing Pocket TTS, Piper, and Kokoro benchmark, then add two
tracks:

1. **Kokoro or Pocket TTS for the low-risk runtime path.** Kokoro is Apache-2.0
   and has many small preset voices. Pocket TTS is MIT code, a 100M CPU model,
   and its official README documents streaming and roughly 200 ms to first
   audio on the authors' hardware. Neither has a real emotion control input.
   Use authored per-state references with Pocket, or choose a fixed cast and
   apply carefully tested post-processing.
2. **IndexTTS-2.5 and Qwen3-TTS-0.6B for expressive experiments.** IndexTTS-2.5
   has a speaker reference plus an 8-value emotion vector, an emotion reference,
   text-derived emotion, intensity, and speed controls. Qwen3-TTS has explicit
   instruction control, voice design, 3-second cloning, and streaming claims.
   Both are plausible on 8 GB only with the small/half-precision configurations
   and careful memory measurement. Their model licenses need legal review before
   committing to a distributable project.

Do not make Fish Audio S2 Pro the default: its official model is 4B parameters
and its published latency is for an H200. It has the most direct inline emotion
tags, but 8 GB VRAM and 16 GB RAM are a poor fit unless a supported quantized
deployment is demonstrated. CosyVoice3, VoxCPM2, and F5-TTS are useful
secondary experiments, not assumptions about interactive performance.

## What counts as emotion control

- **Explicit control** means an API or documented input changes emotion/style
  independently of the target sentence, such as an emotion vector, a named
  emotion reference, an instruction field, or inline tags.
- **Reference prompting** means the system conditions on an audio clip. It may
  copy that clip's emotion, but it is not a reliable emotion selector. A neutral
  speaker reference does not become angry because the application labels it
  "angry".
- **Voice identity** means a speaker embedding, preset voice, or reference clip.
  It does not imply control of delivery.

## Candidate comparison

| Candidate | Voices and emotion/style control | Streaming, cancellation, and fit | License and assessment |
|---|---|---|---|
| **Pocket TTS** | Catalog voices and arbitrary WAV voice cloning. Reference audio can carry a delivery style, but the official README says there is no separate emotion parameter. | Officially CPU-oriented, 100M parameters, audio streaming, and about 200 ms first chunk on the authors' CPU. The repository exposes Python generation and generators, so cancellation can be built around the producer, but application-level cancellation must be tested. **Yes, very likely**, and CPU use leaves the GPU free. | Repository code is MIT. Voice files have separate licenses and some are gated, so audit every chosen voice. Best baseline for small local runtime. Official sources: [repository](https://github.com/kyutai-labs/pocket-tts), [model card](https://huggingface.co/kyutai/pocket-tts), [voice licenses](https://huggingface.co/kyutai/tts-voices). |
| **Piper** | Fixed trained voice per model. No documented emotion, style, or cloning control. Pitch, rate, and volume are not equivalent to emotion. | Fast local ONNX engine with CLI, Python, C/C++, HTTP, and streaming-oriented use. **Yes**, including CPU, and it is the safest latency fallback. The original `rhasspy/piper` repository is archived; development moved to OHF-Voice. | Current engine repository is GPL-3.0. Individual voice model licenses vary and must be checked. **Not a clean choice if North Neeladesh cannot distribute GPL code.** Sources: [current repository](https://github.com/OHF-Voice/piper1-gpl), [voice docs](https://github.com/OHF-Voice/piper1-gpl/blob/main/docs/VOICES.md), [archived original](https://github.com/rhasspy/piper). |
| **Kokoro-82M** | 82M model with many preset voices and speed control. No explicit emotion input and no zero-shot voice cloning in the official API. | Generator yields audio by text chunks, which is suitable for early playback. **Yes**, comfortably on this hardware, CPU or GPU. Cancellation should be straightforward between chunks, but measure the first yielded chunk and whether phonemization blocks it. | Model card and inference library say Apache-2.0 weights/code. Training data includes CC BY material, so keep attribution and review voice files. Strong small, permissive baseline, but emotion is indirect. Sources: [repository](https://github.com/hexgrad/kokoro), [model card](https://huggingface.co/hexgrad/Kokoro-82M). |
| **StyleTTS2** | Style diffusion can produce style without reference speech, and the released multispeaker inference uses reference audio for speaker/style adaptation. No simple runtime emotion API in the official repository. | Older research code with a multi-stage pipeline. The official repo does not present a supported streaming production path. **Inference likely fits** on 8 GB with a small configuration, but RAM, dependencies, and first-audio latency are uncertain. | Code is MIT, but pretrained-model use carries a notice/voice permission requirement, and inference depends on a GPL package in the official instructions. Treat this as a research or fine-tuning base, not a ready runtime. Sources: [repository](https://github.com/yl4579/StyleTTS2), [license and model notice](https://github.com/yl4579/StyleTTS2#license). |
| **F5-TTS** | Zero-shot voice cloning from reference audio. The official UI lists multi-style/multi-speaker generation, but this is reference/style conditioning, not a documented numeric emotion control. | Flow matching with configurable inference steps and chunk inference. Official L20 results are not target-laptop results. **Likely yes** for the 0.3B base in fp16, subject to VRAM and first-chunk measurement; diffusion step count may make short-line startup less attractive. | MIT code, but pretrained models are CC-BY-NC because of Emilia training data. That is a blocker for anything beyond noncommercial university research unless the specific weights have compatible terms. Sources: [repository](https://github.com/SWivid/F5-TTS), [model page](https://huggingface.co/SWivid/F5-TTS). |
| **CosyVoice** | CosyVoice3 supports zero-shot cloning, cross-lingual cloning, pronunciation control, and natural-language instructions including emotion, speed, and volume. These are instruction controls, not a fixed emotion vector. | Official project supports text-in and audio-out streaming and reports latency as low as 150 ms, but on unspecified target hardware. 0.5B models are **plausible on 8 GB**; 1.5B/3.0 variants need measurement and likely quantization/offload. Cancellation needs testing around the async generator/server. | Repository is Apache-2.0, but model and auxiliary resource terms must be checked separately. Strong candidate if 0.5B streaming fits. Sources: [repository](https://github.com/FunAudioLLM/CosyVoice), [CosyVoice3 model](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512). |
| **Fish Speech / Fish Audio S2 Pro** | Direct inline natural-language tags such as `[angry]`, `[whisper]`, `[excited]`, pauses, laughs, and free-form descriptions. It also clones from 10-30 seconds and supports multi-speaker input. This is the clearest documented emotion interface in the shortlist. | S2 Pro is 4B parameters. Official figures are about 100 ms first audio and RTF 0.195 on one H200, not this laptop. **No for an unquantized 8 GB deployment; uncertain with quantization** until the official runtime and memory use are verified. | Code and weights use the Fish Audio Research License, not a permissive standard license. This is a legal blocker for a university project until its restrictions are accepted. Sources: [repository](https://github.com/fishaudio/fish-speech), [license](https://github.com/fishaudio/fish-speech/blob/main/LICENSE), [official docs](https://speech.fish.audio/). |
| **Parler-TTS Mini** | Natural-language description controls speaker characteristics, gender, pitch, speaking rate, recording quality, and style. It has 34 named speakers for consistency. This is prompt-based style control, not an emotion vector or guaranteed emotion label. | 880M model, with SDPA, Flash Attention 2, compile, and streaming guidance. **Possibly yes** in fp16 on 8 GB, but 16 GB RAM and first-audio latency need measurement. The 2.3B Large model is not a sensible first target. | Repository and model family are Apache-2.0 according to the official repository/model pages. Good controlled-style experiment, but likely slower and less robust than an autoregressive streaming model. Sources: [repository](https://github.com/huggingface/parler-tts), [Mini model](https://huggingface.co/parler-tts/parler-tts-mini-v1), [inference guide](https://github.com/huggingface/parler-tts/blob/main/INFERENCE.md). |
| **OpenVoice V1/V2** | Reference voice cloning plus documented controls for emotion, accent, rhythm, pauses, and intonation. The control is exposed through the style/reference pipeline, not a small standardized emotion API. | Lightweight enough that **inference should fit**, but the official project is notebook-oriented and does not promise low-latency streaming. It depends on a TTS base model and conversion stages, so measure first audio and cancellation end to end. | Official repository says V1 and V2 are MIT and free for research and commercial use. Check the licenses of the base TTS and reference voices separately. Useful for offline voice conversion/casting, less attractive for NPC first-audio latency. Source: [repository](https://github.com/myshell-ai/OpenVoice). |
| **Spark-TTS 0.5B** | Zero-shot cloning and virtual voice creation with gender, pitch, and speaking-rate controls. The official interface does not document a direct emotion parameter. | Qwen2.5-based single-stream model. **Likely fits** in fp16 on 8 GB, but its official L20 latency is not transferable. The released inference path is file-oriented; streaming and cancellation need implementation and measurement. | Repository is Apache-2.0, while the model page and training data terms still need recording for redistribution. Worth testing if voice creation matters more than explicit emotion. Source: [repository](https://github.com/SparkAudio/Spark-TTS), [model](https://huggingface.co/SparkAudio/Spark-TTS-0.5B). |
| **Qwen3-TTS** | 0.6B and 1.7B models. CustomVoice accepts an instruction field for tone/emotion, VoiceDesign creates a voice from a description, and Base clones a voice from about 3 seconds. This is explicit instruction control for style, plus reference cloning. | Official repository claims streaming, first packet after one character, and 97 ms end-to-end synthesis latency, but those are vendor measurements. **0.6B is the strongest likely fit** on 8 GB in fp16; 1.7B may fit only with careful dtype/attention settings. Python generation and streaming cancellation behavior need direct tests. | Repository and model pages state Apache-2.0, but confirm each exact checkpoint's card before redistribution. This is the best new candidate for the project if a local 0.6B run meets memory and first-chunk gates. Source: [repository](https://github.com/QwenLM/Qwen3-TTS), [model collection](https://huggingface.co/collections/Qwen/qwen3-tts). |
| **VoxCPM2** | 2B model with voice design from text, controllable cloning, style guidance for emotion/pace/expression, and a separate continuation mode that preserves reference emotion/style. The first two are prompt controls, not a compact emotion API. | Official Python API has `generate_streaming`; published RTF is about 0.3 on RTX 4090 and the model table lists about 8 GB VRAM. **Borderline/no for an 8 GB laptop** after framework overhead; VoxCPM1.5/0.5B are more plausible, but the older models have less control. Test the official or GGUF runtime rather than assuming the table fits. | Official repository says Apache-2.0 for code and weights. Strong candidate for later hardware tests, especially with the official llama.cpp-omni route, but not the first default. Source: [repository](https://github.com/OpenBMB/VoxCPM). |
| **IndexTTS-2.5** | Single reference speaker plus an 8-float emotion vector `[happy, angry, sad, afraid, disgusted, melancholic, surprised, calm]`, emotion reference audio, `emo_alpha`, text-derived emotion, explicit emotion text, pronunciation controls, and `duration_factor` speed. This is the most operationally precise emotion control found. | Official project reports around 0.206 RTF on RTX 4090 with BF16, but no first-audio streaming claim. **Likely fits** in 8 GB with BF16 and its 0.8B checkpoint, though CUDA kernels and model/runtime overhead must be measured. It currently looks like a full-line generator, so cancellation may discard partial work. | Uses the Bilibili Model Use License Agreement, not Apache/MIT. The repository also includes a disclaimer and asks commercial users to contact the team. **Legal review required before adopting.** Source: [repository](https://github.com/index-tts/index-tts), [license](https://github.com/index-tts/index-tts/blob/main/LICENSE). |

## Hardware and runtime risks

The RTX 5060 Laptop's 8 GB is a hard limit, not the model's advertised
parameter count. Weights, CUDA workspaces, attention cache, vocoder, Python,
and any ASR or LLM running beside TTS all compete for it. The project also has
only about 16 GB system RAM, so loading two large models or swapping between
voices is not free.

For the first benchmark pass, keep one process resident per candidate and use
one short line, one medium line, and a cancellation line. Test fp16 and bf16
where supported, then CPU for Pocket/Kokoro/Piper. Record:

- cold model load and warm request time;
- time from request to first playable PCM chunk;
- generated audio duration, real-time factor, and peak VRAM/RAM;
- whether chunks are monotonic and gap-free when played immediately;
- cancellation latency and whether a cancelled request leaves GPU memory or a
  worker occupied;
- repeated-voice similarity across 20 lines per NPC;
- emotion recognition by blind human ratings, using identical text and speaker
  references;
- text accuracy for names, numbers, pronunciation overrides, and short
  interrupted lines.

For streaming candidates, the application should split LLM output at authored
speech boundaries and cancel the producer when the NPC state changes. Do not
assume that a library's Python generator is cancellable merely because it
returns chunks. Verify that the underlying GPU operation stops promptly.

## Licensing checklist

"Open source" in a repository README is not enough for the complete artifact.
Before packaging a university build, record the license and attribution for:

- code;
- each model checkpoint;
- each preset or reference voice;
- phonemizers, vocoders, CUDA extensions, and server runtimes;
- any training-data restriction or noncommercial clause.

The clearest permissive choices in this survey are Kokoro (Apache-2.0),
OpenVoice (MIT), Spark-TTS (Apache-2.0), CosyVoice (Apache-2.0), VoxCPM
(Apache-2.0), and Pocket TTS code (MIT), subject to checkpoint and voice-file
verification. F5-TTS weights are CC-BY-NC, Piper is GPL-3.0, Fish Audio uses a
custom research license, and IndexTTS uses a custom Bilibili license. These are
not interchangeable university-project permissions.

## Primary sources

All technical and licensing claims above come from the official project or
model pages linked in the comparison table. The local project context is the
existing [half-duplex voice benchmark](../../prototypes/half-duplex-voice-benchmark/README.md),
which already measures Piper, Kokoro, and Pocket TTS first-chunk behavior and
tests Pocket voice casting. No target-laptop performance is claimed here.
