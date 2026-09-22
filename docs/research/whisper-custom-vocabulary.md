# Custom vocabulary with whisper.cpp

## Findings

The official `whisper-cli` interface does not provide a true hotword or
weighted vocabulary option. Its `--prompt` option is an **initial prompt**,
limited to half of the model text context. It is decoder context, not a
dictionary, so supplying a long bare list can bias ordinary speech.

The official CLI does expose `--grammar`, but that constrains the complete
decoded output to a GBNF grammar. It is appropriate for a closed command
language, not unrestricted telephone speech with occasional place names.

The official Whisper API describes `initial_prompt` similarly: text supplied
to guide transcription, including proper nouns and styles. It does not promise
forced spelling or correction.

## Recommendation for this project

1. Keep the normal transcription pass unprompted.
2. For a narrowly detected routing/location request, optionally retry with a
   short, natural initial prompt containing only the relevant proper nouns.
3. Do not pass generic phrases through the prompt and do not rewrite arbitrary
   transcripts with aliases.
4. If exact recognition is required, use a larger/better English Whisper model
   or a separate domain recognizer/post-processing layer; whisper.cpp's CLI has
   no native hotword bias mechanism.

## Sources

- whisper.cpp CLI documentation and options:
  https://github.com/ggml-org/whisper.cpp/blob/master/examples/cli/README.md
- whisper.cpp CLI parameter implementation (`--prompt`, `--grammar`, and
  `--carry-initial-prompt`):
  https://github.com/ggml-org/whisper.cpp/blob/master/examples/cli/cli.cpp
- OpenAI Whisper decoding options and `initial_prompt` guidance:
  https://github.com/openai/whisper/blob/main/whisper/transcribe.py
