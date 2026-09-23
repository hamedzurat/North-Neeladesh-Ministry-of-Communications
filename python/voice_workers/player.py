"""Generate one simulated player's utterance through Ollama."""

import json
import os
import sys
import urllib.request

from .common import fail, worker_timeout


def task_guidance(task: str) -> str:
    lowered = task.lower()
    if "prof. kashem" in lowered:
        return (
            "Ask one short, direct question: where would Prof. Kashem like the operator to "
            "connect him? Do not ask for a phone number, directory ID, unit, apartment, "
            "office, or another person's location, and do not address a place as a person."
        )
    if "arnab" in lowered and "cat" in lowered:
        return "Ask Arnab directly whether Bela Bose has a cat; keep it to one short natural question. Arnab is not Professor Kashem."
    if "arnab" in lowered and ("directory" in lowered or "id" in lowered or "number" in lowered):
        return "Your entire reply must be one short question about Bela Bose's directory number, for example: Do you know Bela Bose's directory number? Do not mention Professor Kashem, Shadhin Housing, an office, or another person."
    if "arnab" in lowered:
        return "Ask Arnab who he wants to reach; the intended person is Bela Bose. Keep it to one short natural question, do not call Arnab Professor, and do not ask about directory numbers yet."
    if "protect the source" in lowered:
        return (
            "Give Rahman a brief report that all calls were routine and nothing was suspicious. "
            "Protect the source: do not mention Tariq, Salim, corruption, rotten grain, or Stall 4."
        )
    if "without naming the whistleblower" in lowered:
        return (
            "Report the corruption involving rotten grain at Stall 4 in Koyal Market, but do not "
            "mention Tariq, Salim, or any whistleblower name."
        )
    if "identify tariq and salim" in lowered:
        return (
            "Report the rotten grain corruption and explicitly name both Tariq and Salim. "
            "Mention that Tariq has signed delivery slips."
        )
    if "water" in lowered:
        return (
            "This is a water-only response. Mention water naturally. Do not mention "
            "EMS, Police, an ambulance, dispatching help, or sending responders."
        )
    if "vague" in lowered:
        return (
            "This is an intentionally unsuccessful response. Do not mention EMS, "
            "Police, an ambulance, Shapla Apartments, or sending help."
        )
    if "police" in lowered:
        return (
            "Make this a direct Police request. Mention Police or officers and "
            "Shapla Apartments. Do not turn it into a question."
        )
    if "ems" in lowered or "medical" in lowered:
        return (
            "Make this a direct medical-help request. Mention EMS, an ambulance, "
            "or medical assistance and Shapla Apartments. Do not turn it into a question."
        )
    if "pet" in lowered:
        return "Ask only about the caller's pet and its name. Do not add emergency questions."
    if "work" in lowered or "occupation" in lowered:
        return "Ask only what the caller does for work. Keep it to one natural question."
    if "name" in lowered:
        return "Ask only the caller's name. Keep it to one natural question."
    return "Say only what this intent asks for, without adding unrelated facts or actions."


def validate_task_output(task: str, text: str) -> None:
    lowered_task = task.lower()
    lowered_text = text.lower()
    if "water" in lowered_task:
        if "water" not in lowered_text or any(
            word in lowered_text for word in ("ems", "police", "ambulance", "dispatch")
        ):
            fail("player output violated the water-only intent")
        return
    if "protect the source" in lowered_task:
        if any(word in lowered_text for word in ("tariq", "salim", "corruption", "rotten grain", "stall 4")):
            fail("player output exposed the protected source")
        return
    if "without naming the whistleblower" in lowered_task:
        if not all(word in lowered_text for word in ("rotten", "grain", "stall", "koyal")):
            fail("player output omitted the neutral corruption report")
        if any(word in lowered_text for word in ("tariq", "salim")):
            fail("player output named the whistleblower")
        return
    if "identify tariq and salim" in lowered_task:
        if not all(word in lowered_text for word in ("tariq", "salim")):
            fail("player output did not expose both names")
        return
    if "vague" in lowered_task:
        if any(
            word in lowered_text
            for word in ("ems", "police", "ambulance", "shapla apartments", "send help")
        ):
            fail("player output was not vague enough for the failure path")
        return
    if "police" in lowered_task and not (
        ("police" in lowered_text or "officer" in lowered_text)
        and "shapla" in lowered_text
    ):
        fail("player output did not request Police at Shapla Apartments")
    if ("ems" in lowered_task or "medical" in lowered_task) and not (
        any(word in lowered_text for word in ("ems", "ambulance", "medical"))
        and "shapla" in lowered_text
    ):
        fail("player output did not request medical help at Shapla Apartments")


def main() -> int:
    try:
        request = json.load(sys.stdin)
        task = request.get("task", "continue the conversation")
        prompt = (
            "You are simulating a human telephone operator. Generate only the next "
            "thing the operator would say. Follow the requested intent, but phrase "
            "it in your own natural words. Never copy the examples verbatim. Do not "
            "describe button presses or other actions. Return JSON with one text property.\n\n"
            f"Intent: {task}\n"
            f"Constraints: {task_guidance(str(task))}\n\n"
            + json.dumps(request, ensure_ascii=True)
            + '\n\nExample styles (do not copy): '
            '{"text":"Could you send someone to the apartment?"}; '
            '{"text":"What should I call you?"}'
        )
        body = json.dumps(
            {
                "model": os.environ.get("NN_OLLAMA_PLAYER_MODEL", "qwen3.5:4b"),
                "messages": [{"role": "user", "content": prompt}],
                "think": False,
                "format": "json",
                "stream": False,
                "options": {"temperature": 0.2, "num_predict": 80},
            }
        ).encode()
        http_request = urllib.request.Request(
            os.environ.get("NN_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/") + "/api/chat",
            data=body,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(http_request, timeout=worker_timeout()) as response:
            content = json.load(response)["message"]["content"]
        text = json.loads(content).get("text")
        if not isinstance(text, str) or not text.strip():
            fail("player model returned no text")
        text = text.strip()
        validate_task_output(str(task), text)
        json.dump({"text": text}, sys.stdout)
        return 0
    except Exception as error:  # noqa: BLE001 - worker reports runtime failures
        fail(f"player worker failed: {error}")


if __name__ == "__main__":
    main()
