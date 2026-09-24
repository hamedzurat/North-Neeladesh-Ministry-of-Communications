"""Generic one-word Ollama classifier for story and test workers."""

import json
import os
import sys
import urllib.request

from .common import fail, worker_timeout


def main() -> int:
    try:
        request = json.load(sys.stdin)
        prompt = request["prompt"]
        text = request["text"]
        if not isinstance(prompt, str) or not isinstance(text, str) or not text.strip():
            fail("classifier request must contain prompt and text")
        full_prompt = f"{prompt}\n\nPlayer text:\n{text}\n\nReturn JSON only. Example: {{\"classification\":\"success\"}}"
        body = json.dumps(
            {
                "model": os.environ.get("NN_OLLAMA_CLASSIFIER_MODEL", "qwen3.5:4b"),
                "messages": [{"role": "user", "content": full_prompt}],
                "think": os.environ.get("NN_OLLAMA_THINK", "false").lower() in {"1", "true", "yes"},
                "stream": False,
                "options": {
                    "temperature": 0.0,
                    "num_predict": int(os.environ.get("NN_OLLAMA_CLASSIFIER_NUM_PREDICT", "8")),
                },
            }
        ).encode()
        request = urllib.request.Request(
            os.environ.get("NN_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/") + "/api/chat",
            data=body,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=worker_timeout()) as response:
            result = json.load(response)
        content = result["message"]["content"].strip()
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
