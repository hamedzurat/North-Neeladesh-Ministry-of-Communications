"""Shared Ollama chat transport for the task-specific voice workers."""

import json
import os
import urllib.request

from .common import worker_timeout


def chat(
    *,
    model: str,
    messages: list[dict[str, str]],
    options: dict[str, object],
    response_format: str = "json",
    keep_alive: int | None = None,
) -> str:
    payload: dict[str, object] = {
        "model": model,
        "messages": messages,
        "think": os.environ.get("NN_OLLAMA_THINK", "false").lower() in {"1", "true", "yes"},
        "format": response_format,
        "stream": False,
        "options": options,
    }
    if keep_alive is not None:
        payload["keep_alive"] = keep_alive
    request = urllib.request.Request(
        os.environ.get("NN_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/") + "/api/chat",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=worker_timeout()) as response:
        result = json.load(response)
    content = result.get("message", {}).get("content")
    if not isinstance(content, str):
        raise ValueError("Ollama returned no message content")
    return content
