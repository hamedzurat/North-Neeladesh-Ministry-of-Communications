# Automatic-turn audio-session prototype

> **THROWAWAY PROTOTYPE** — this material exists only to decide the audio-session
> state model in the Wayfinder ticket. It is not game code.

This prototype asks whether the selected Qwen3 4B dialogue model can react
naturally to a hard interruption when the next Response Context distinguishes:

- speech confirmed as played to the Operator;
- a private unfinished thought that was not heard; and
- the interruption event and the Operator's next utterance.

Run `benchmark_interruption_context.py` against the same OpenAI-compatible
`llama-server` used by the half-duplex benchmark. Keep result JSON outside the
repository because generated conversations are diagnostic data.

