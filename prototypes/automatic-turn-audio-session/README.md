# Automatic-turn audio-session prototype

> **THROWAWAY PROTOTYPE** — this material exists only to decide the audio-session
> state model in the Wayfinder ticket. It is not game code.

This prototype asks whether the selected Qwen3 4B dialogue model can react
naturally to a hard interruption when the next Response Context contains:

- speech confirmed as played to the Operator;
- the interruption event;
- the Subscriber's ordinary knowledge and current goal; and
- the Operator's next utterance.

An initial version also supplied the verbatim remainder of the interrupted
sentence as a private unfinished thought. It passed 15 of 20 probes, but failed
all five topic-change probes by resuming obsolete speech instead of answering
the Operator's new question. The accepted state model therefore omits that
remainder. The core records the interruption and completed audible sentences;
the next response uses the normal Call Premise, Subscriber knowledge, and newest
Operator utterance. Natural restatement is allowed but never guaranteed or
implemented as a special memory feature.

Run `benchmark_interruption_context.py` against the same OpenAI-compatible
`llama-server` used by the half-duplex benchmark. Keep result JSON outside the
repository because generated conversations are diagnostic data.

Open `audio-session.html` directly in a browser for free play and guided
walkthroughs of ordinary PTT, hard interruption, automatic half duplex, and a
rejected overlapping-capture attempt.
