"""Generic one-word Ollama classifier for story and test workers."""

import json
import os
import sys

from .common import fail, prompt_template
from .ollama import chat


def main() -> int:
    try:
        request = json.load(sys.stdin)
        prompt = request["prompt"]
        text = request["text"]
        if not isinstance(prompt, str) or not isinstance(text, str) or not text.strip():
            fail("classifier request must contain prompt and text")
        system = prompt_template("classifier_system.txt").format(story_rules=prompt)
        content = chat(
            model=os.environ.get("NN_OLLAMA_CLASSIFIER_MODEL", "qwen3.5:4b"),
            messages=[{"role": "system", "content": system}, {"role": "user", "content": text}],
            options={
                "temperature": 0.0,
                "num_predict": int(os.environ.get("NN_OLLAMA_CLASSIFIER_NUM_PREDICT", "8")),
            },
        ).strip()
        try:
            parsed = json.loads(content)
            value = str(parsed["classification"]).strip().lower()
        except (ValueError, KeyError, TypeError):
            value = content.split()[0].strip("`.,:;\"'").lower()
        json.dump({"classification": value}, sys.stdout)
        return 0
    except Exception as error:  # noqa: BLE001 - worker reports runtime failures
        fail(f"classifier failed: {error}")


if __name__ == "__main__":
    main()
