# Story workspace

The authored setting, characters, facts, and story rules have been removed.

The runtime still provides the telephone exchange, call routing, shifts, the
LLM dialogue worker, voice transport, and the text test frontend. New story
content belongs in the story configuration and story state implementation.

The LLM may generate dialogue only. It does not create facts, change routing,
or mutate authoritative game state.
