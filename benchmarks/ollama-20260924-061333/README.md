# Ollama model comparison

Generated: 20260924-061333

Each profile ran every story-test path with a fresh backend. The complete test
transcript is in each profile's transcript.log; per-path results are in
paths.tsv; startup and runner diagnostics are in backend.log and runner.log.
summary.tsv records
wall time and whether the suite completed successfully.

The model is selected consistently for dialogue, classifier, and simulated
player workers. NN_OLLAMA_THINK controls Ollama's thinking mode.
