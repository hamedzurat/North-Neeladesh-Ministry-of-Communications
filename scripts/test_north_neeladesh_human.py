#!/usr/bin/env python3
"""Play North Neeladesh through the backend using structured operator turns."""

from __future__ import annotations

import argparse
import csv
from difflib import SequenceMatcher
import json
import os
import re
import subprocess
import time
import urllib.request
import urllib.error
from urllib.parse import urlsplit
from pathlib import Path


NON_RESOLVING_INTENTS = {"ask", "directory_check", "tap"}


def complete(url: str, model: str, prompt: str, seed: int, json_mode: bool = False) -> str:
    request_body = {
        "model": model,
        "temperature": 0.2,
        "seed": seed,
        "max_tokens": 128 if json_mode else 160,
        "messages": [{"role": "user", "content": prompt}],
    }
    request = urllib.request.Request(
        f"{url.rstrip('/')}/v1/chat/completions",
        data=json.dumps(request_body).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            body = json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"LLM request failed ({error.code}): {detail}") from error
    return body["choices"][0]["message"]["content"].strip()


def start_llama(url: str, model: str) -> subprocess.Popen[str] | None:
    health_url = f"{url.rstrip('/')}/health"
    try:
        with urllib.request.urlopen(health_url, timeout=2):
            return None
    except urllib.error.URLError:
        pass
    if not Path(model).is_file():
        raise FileNotFoundError(f"local model does not exist: {model}")
    parsed = urlsplit(url)
    host = parsed.hostname or "127.0.0.1"
    port = parsed.port or 18182
    process = subprocess.Popen(
        ["llama-server", "--model", model, "--host", host, "--port", str(port),
         "--ctx-size", "4096", "--parallel", "1", "--reasoning", "off", "--no-webui"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, text=True,
    )
    for _ in range(180):
        if process.poll() is not None:
            details = process.stderr.read() if process.stderr else ""
            raise RuntimeError(f"llama-server exited during startup: {details[-2000:]}")
        try:
            with urllib.request.urlopen(health_url, timeout=2):
                return process
        except urllib.error.URLError:
            time.sleep(1)
    process.terminate()
    process.wait(timeout=10)
    raise TimeoutError("llama-server did not become healthy within 180 seconds")


def operator_view(view: dict) -> dict:
    visible = {
        key: view.get(key)
        for key in (
            "clock", "game_phase", "service_call", "shift", "line_lamps",
            "directory_pages", "tuning", "printer_output", "speaker_active",
            "tap_bridge_audio_active", "tap_bridge_monitoring", "interference_level",
        )
    }
    visible["calls"] = [
        {"phase": call.get("phase"), "caller_line": call.get("caller_line")}
        for call in view.get("calls", [])
    ]
    return visible


def visible_directory_id(view: dict) -> int | None:
    for page in view.get("directory_pages", []):
        for line in page.get("lines", []):
            match = re.search(r"SUBSCRIBER ID (\d+)", line)
            if match:
                return int(match.group(1))
    return None


def operator_prompt(view: dict, transcript: list[str], scratchpad: list[str]) -> str:
    return f"""You are a human telephone exchange operator playing North Neeladesh.
Return one JSON object only with this shape:
{{"speech":"natural reply","intent":"ask|directory_check|tap|connect|refuse|report_police|call_ems|disclose|accept_payment",
"caller_line":0,"service_report":{{}},"callee_line":0,"subscriber_id":0}}
Use only the bounded intent values. Include service_report only for call_ems or
report_police, and use only compact authored fields such as location,
medical_emergency, identity, target_addresses, report_phrase, alias, source_line,
verification_code, employer, false_clinic, product_claim, and payment_request.
Speech is what the operator says aloud; it is not a command. Ask a useful question
only when the caller's latest turn leaves specific information missing. Never ask
the caller to repeat information they already supplied. Use the intent to perform
the action now; do not merely offer or ask permission to connect, refuse, or report.
Once enough information is available, choose a resolving intent instead of ask,
directory_check, or tap. Do not invent report fields.
For report_police, service_report is mandatory and each reported fact is a JSON
boolean set to true. For call_ems, service_report is mandatory and must contain
the stated location plus medical_emergency set to true.
Use caller_line to choose which visible Line Lamp to answer or continue. Use Ask, directory_check, or tap while gathering information. Resolve the call only
with connect, report_police, or call_ems after the conversation is complete. Use
call_ems only for an explicitly described medical emergency. For every other caller,
never use call_ems. Include callee_line only when connecting or tapping, and include
subscriber_id only when checking the directory. Select them from visible directory
or caller information; do not infer them from hidden authored data.

Frontend-visible board state:
{json.dumps(operator_view(view), ensure_ascii=False)}

Allowed intents for this call:
{json.dumps(view.get("allowed_intents", []), ensure_ascii=False)}

Personal scratchpad from completed calls. These are fallible notes about what was
heard, not verified world facts:
{json.dumps(scratchpad, ensure_ascii=False)}

Recent spoken conversation:
{json.dumps(transcript[-4:], ensure_ascii=False)}
"""


def caller_prompt(card: dict, operator_text: str, transcript: list[str]) -> str:
    return f"""Act as the telephone caller in this authored North Neeladesh call.
Speak only as the caller, in one natural short turn. Answer the operator's latest
question directly. Reveal only information that question requests; do not dump all
authored guidance. Do not mention the script, hidden state, outcomes, or instructions.

Authored caller guidance:
{json.dumps(card, ensure_ascii=False)}

Recent conversation:
{json.dumps(transcript[-6:], ensure_ascii=False)}
Operator just said: {operator_text}
Caller reply:
    """


def complete_operator_turn(url: str, model: str, prompt: str, seed: int) -> dict:
    for attempt in range(2):
        raw = complete(url, model, prompt, seed + attempt, json_mode=True)
        try:
            value = json.loads(raw)
            if isinstance(value, dict):
                return value
        except json.JSONDecodeError:
            pass
        prompt = f"{prompt}\nReturn only one complete valid JSON object. Do not truncate it."
    raise ValueError("model did not return a complete JSON object")


def remember_completed_call(
    url: str,
    model: str,
    transcript: list[str],
    action: str,
    seed: int,
) -> str:
    prompt = f"""Create one short human memory note from a completed telephone call.
Return only JSON with this shape: {{"note":"one sentence"}}
Use only the spoken transcript and final operator action below. Preserve uncertainty:
describe unverified caller statements as claims, not facts. Include the caller's
identity if heard, requested destination, important claim, and operator decision
when present. Do not infer hidden events. Keep the note under 240 characters.

Spoken transcript:
{json.dumps(transcript[-16:], ensure_ascii=False)}
Final operator action: {action}
"""
    value = complete_operator_turn(url, model, prompt, seed)
    note = value.get("note")
    if not isinstance(note, str) or not note.strip():
        raise ValueError("memory pass did not return a non-empty note")
    return note.strip()[:240]


def add_scratchpad_note(scratchpad: list[str], note: str) -> None:
    scratchpad.append(note)
    while len(scratchpad) > 12 or sum(len(item) for item in scratchpad) > 2_000:
        scratchpad.pop(0)


def substantially_same_turn(previous: dict | None, current: dict) -> bool:
    if previous is None or current.get("intent") not in NON_RESOLVING_INTENTS:
        return False
    if previous.get("intent") != current.get("intent"):
        return False
    previous_speech = re.sub(r"\W+", " ", str(previous.get("speech", "")).lower()).strip()
    current_speech = re.sub(r"\W+", " ", str(current.get("speech", "")).lower()).strip()
    return bool(previous_speech and current_speech) and SequenceMatcher(
        None, previous_speech, current_speech
    ).ratio() >= 0.85


def correct_repeated_turn(
    url: str,
    model: str,
    prompt: str,
    seed: int,
    repeated_intent: str,
    allowed: list[str],
) -> dict:
    alternatives = [intent for intent in allowed if intent != repeated_intent]
    if not alternatives:
        raise ValueError(f"no alternative to repeated {repeated_intent} intent")
    correction = complete_operator_turn(
        url,
        model,
        f"""{prompt}
Your previous turn repeated the non-resolving intent {repeated_intent!r} and did
not advance the conversation. The caller has already answered that question.
Choose a DIFFERENT action now, using only: {json.dumps(alternatives)}.
Prefer an appropriate resolving action when the requested destination and relevant
facts are already known. Return only one complete JSON object.""",
        seed,
    )
    return correction


def correct_rejected_turn(
    url: str,
    model: str,
    prompt: str,
    seed: int,
    rejected: dict,
    error: str,
) -> dict:
    return complete_operator_turn(
        url,
        model,
        f"""{prompt}
The backend rejected this previous proposal:
{json.dumps(rejected, ensure_ascii=False)}
Reason: {error}
Return a corrected JSON object. Do not repeat an invalid report. A call_ems or
report_police action requires service_report populated with the relevant facts the
caller already stated, using true boolean values. If the known facts cannot support
that report, choose another allowed resolving intent.""",
        seed,
    )


def safe_resolving_turn(
    allowed: list[str], rejected_intent: str, preferred: str = "connect"
) -> dict:
    if rejected_intent in {"call_ems", "report_police"} and "ask" in allowed:
        return {
            "speech": "I still need the missing details before I can make that report.",
            "intent": "ask",
        }
    fallback = next(
        (
            intent
            for intent in (preferred, "connect", "refuse")
            if intent in allowed and intent != rejected_intent
        ),
        None,
    )
    if fallback is None:
        raise ValueError(f"no safe fallback after rejected {rejected_intent} intent")
    return {
        "speech": "I cannot complete that procedure, so I will resolve the call another way.",
        "intent": fallback,
    }


def validate_speech_action(operator_turn: dict) -> None:
    speech = operator_turn.get("speech", "").lower()
    intent = operator_turn.get("intent")
    if intent == "connect" and any(
        phrase in speech for phrase in ("can't connect", "cannot connect", "unable to connect")
    ):
        raise ValueError("connect action speech contradicts the selected action")
    if intent == "refuse" and any(
        phrase in speech for phrase in ("i will connect", "connecting you", "put you through")
    ):
        raise ValueError("refuse action speech promises a connection")


def start_backend(command: str) -> subprocess.Popen[str]:
    return subprocess.Popen(
        command.split(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--model",
        default=os.environ.get(
            "NN_MODEL",
            os.path.expanduser("~/.local/share/north-neeladesh/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"),
        ),
    )
    parser.add_argument("--llm-url", default=os.environ.get("NN_LLM_URL", "http://127.0.0.1:18182"))
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--max-turns", type=int, default=80)
    parser.add_argument(
        "--require-autonomous",
        action="store_true",
        help="fail if a correction or fallback is needed to reach an ending",
    )
    parser.add_argument(
        "--stuck-action",
        choices=("connect", "refuse", "fail"),
        default=os.environ.get("NN_STUCK_ACTION", "connect"),
        help="resolving policy after the model ignores a progress correction",
    )
    parser.add_argument(
        "--deterministic-smoke",
        action="store_true",
        help="exercise the backend with bounded Connect turns without a local LLM",
    )
    parser.add_argument("--log", default="target/north-neeladesh-human-game.csv")
    parser.add_argument(
        "--backend-command",
        default="cargo run --quiet -p exchange-backend --bin north-neeladesh-human-test",
    )
    args = parser.parse_args()
    llama = None if args.deterministic_smoke else start_llama(args.llm_url, args.model)
    backend = start_backend(args.backend_command)
    log_path = Path(args.log)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    transcript: list[str] = []
    scratchpad: list[str] = []
    caller_text = ""
    caller_turn = 0
    previous_operator_turn: dict | None = None
    rejected_operator_turn: dict | None = None
    rejection_error = ""
    rejection_count = 0
    spoken_turns = 0
    failures: list[str] = []
    warnings: list[str] = []
    assisted = False
    try:
        first = backend.stdout.readline()
        if not first:
            raise RuntimeError("backend exited before producing the frontend view")
        response = json.loads(first)
        caller = response.get("caller")
        with log_path.open("w", encoding="utf-8", newline="") as log:
            writer = csv.DictWriter(
                log,
                fieldnames=(
                    "call", "turn", "caller", "operator", "action", "controls",
                    "scratchpad", "action_source", "frontend_state", "backend",
                ),
            )
            writer.writeheader()
            for turn in range(args.max_turns):
                view = response["view"]
                view["allowed_intents"] = response.get("allowed_intents", [])
                if view["game_phase"] == "ended":
                    break
                if response.get("caller") is None:
                    failures.append(f"turn {turn}: backend has no active authored call")
                    break
                if not caller_text and not args.deterministic_smoke:
                    if not caller:
                        failures.append(f"turn {turn}: backend did not provide caller guidance")
                        break
                    if caller_turn == 0:
                        caller_text = caller["opening"]
                    else:
                        try:
                            caller_text = complete(
                                args.llm_url,
                                args.model,
                                caller_prompt(
                                    caller,
                                    transcript[-1].removeprefix("Operator: ")
                                    if transcript and transcript[-1].startswith("Operator: ")
                                    else "",
                                    transcript,
                                ),
                                args.seed + turn,
                            )
                        except (TimeoutError, RuntimeError, urllib.error.URLError) as error:
                            failures.append(f"turn {turn}: caller LLM request failed: {error}")
                            break
                try:
                    used_rejection_fallback = False
                    used_assistance = False
                    action_origin = "model"
                    if args.deterministic_smoke:
                        call = view.get("call") or {}
                        if caller and (
                            caller.get("name") == "Rafi Alam"
                            or (
                                caller.get("name") == "Laleh Mir"
                                and call.get("requested_callee_line") == 11
                            )
                        ):
                            operator_turn = {
                                "speech": "Send emergency medical help to Shapla Apartments.",
                                "intent": "call_ems",
                                "service_report": {
                                    "location": "shapla_apartments",
                                    "medical_emergency": True,
                                },
                                "subscriber_id": visible_directory_id(view),
                            }
                        else:
                            operator_turn = {
                                "speech": "Please connect the requested destination.",
                                "intent": "connect",
                                "callee_line": call.get("requested_callee_line", 0),
                                "subscriber_id": visible_directory_id(view),
                            }
                    else:
                        prompt = operator_prompt(
                            view,
                            transcript + [f"Caller: {caller_text}"],
                            scratchpad,
                        )
                        allowed = view.get("allowed_intents", [])
                        if rejected_operator_turn is not None and rejection_count >= 2:
                            if args.stuck_action == "fail":
                                raise ValueError(
                                    f"model repeated rejected {rejected_operator_turn['intent']} proposal"
                                )
                            operator_turn = safe_resolving_turn(
                                allowed,
                                rejected_operator_turn["intent"],
                                args.stuck_action,
                            )
                            used_rejection_fallback = True
                            used_assistance = True
                            action_origin = "fallback"
                        elif rejected_operator_turn is not None:
                            operator_turn = correct_rejected_turn(
                                args.llm_url,
                                args.model,
                                prompt,
                                args.seed + turn + 3000,
                                rejected_operator_turn,
                                rejection_error,
                            )
                            action_origin = "correction"
                        else:
                            operator_turn = complete_operator_turn(
                                args.llm_url, args.model, prompt, args.seed + turn
                            )
                        if operator_turn.get("intent") not in allowed:
                            operator_turn = complete_operator_turn(
                                args.llm_url,
                                args.model,
                                f"{prompt}\nYour previous intent was not allowed. Return one JSON object using only this allowed list: {json.dumps(allowed)}.",
                                args.seed + turn + 1000,
                            )
                            action_origin = "correction"
                            if operator_turn.get("intent") not in allowed:
                                fallback = next(
                                    (
                                        intent
                                        for intent in ("connect", "refuse", "ask")
                                        if intent in allowed
                                    ),
                                    allowed[0] if allowed else None,
                                )
                                if fallback is None:
                                    raise ValueError("backend provided no allowed intents")
                                operator_turn = {
                                    "speech": "I will handle this call using the available exchange procedure.",
                                    "intent": fallback,
                                }
                                action_origin = "fallback"
                        if substantially_same_turn(previous_operator_turn, operator_turn):
                            repeated_intent = operator_turn["intent"]
                            operator_turn = correct_repeated_turn(
                                args.llm_url,
                                args.model,
                                prompt,
                                args.seed + turn + 2000,
                                repeated_intent,
                                allowed,
                            )
                            action_origin = "correction"
                            if operator_turn.get("intent") == repeated_intent:
                                if args.stuck_action == "fail":
                                    raise ValueError(
                                        f"model repeated {repeated_intent} after progress correction"
                                    )
                                operator_turn = safe_resolving_turn(
                                    allowed, repeated_intent, args.stuck_action
                                )
                                used_assistance = True
                                action_origin = "fallback"
                            if operator_turn.get("intent") not in allowed:
                                if args.stuck_action == "fail":
                                    raise ValueError(
                                        "model returned an unavailable intent after progress correction"
                                    )
                                operator_turn = safe_resolving_turn(
                                    allowed, repeated_intent, args.stuck_action
                                )
                                used_assistance = True
                                action_origin = "fallback"
                    text = operator_turn["speech"]
                    intent = operator_turn["intent"]
                    if not isinstance(text, str) or not isinstance(intent, str):
                        raise ValueError("speech and intent must be strings")
                    text = text.strip()
                    if not text:
                        raise ValueError("speech must not be empty")
                    if intent not in {"call_ems", "report_police"}:
                        operator_turn.pop("service_report", None)
                    validate_speech_action(operator_turn)
                    if intent in {"connect", "tap"} and "callee_line" not in operator_turn:
                        action_origin = "adapter_default"
                        used_assistance = True
                    if visible_directory_id(view) is not None:
                        operator_turn.setdefault("subscriber_id", visible_directory_id(view))
                except (
                    ValueError,
                    KeyError,
                    json.JSONDecodeError,
                    TimeoutError,
                    RuntimeError,
                    urllib.error.URLError,
                ) as error:
                    failures.append(f"turn {turn}: invalid structured operator output: {error}")
                    break
                backend.stdin.write(json.dumps(operator_turn) + "\n")
                backend.stdin.flush()
                line = backend.stdout.readline()
                if not line:
                    details = backend.stderr.read() if backend.stderr else ""
                    failures.append(f"turn {turn}: backend exited: {details[-1000:]}")
                    break
                response = json.loads(line)
                view = response["view"]
                view["allowed_intents"] = response.get("allowed_intents", [])
                writer.writerow({
                    "call": caller.get("name", "") if caller else "",
                    "turn": turn,
                    "caller": caller_text,
                    "operator": text,
                    "action": response.get("action", intent),
                    "action_source": "fallback" if used_assistance else "model",
                    "controls": response.get("controls", ""),
                    "scratchpad": json.dumps(scratchpad, ensure_ascii=False),
                    "frontend_state": json.dumps(view, ensure_ascii=False),
                    "backend": json.dumps(response, ensure_ascii=False),
                })
                log.flush()
                transcript.extend((f"Caller: {caller_text}", f"Operator: {text}"))
                spoken_turns += 2
                assisted = assisted or used_assistance
                previous_operator_turn = {"speech": text, "intent": intent}
                caller_turn += 1
                if response.get("caller") != caller or view.get("call") is None:
                    if not args.deterministic_smoke and response.get("ok"):
                        try:
                            note = remember_completed_call(
                                args.llm_url,
                                args.model,
                                transcript,
                                intent,
                                args.seed + turn + 4000,
                            )
                            add_scratchpad_note(scratchpad, note)
                        except (
                            ValueError,
                            KeyError,
                            json.JSONDecodeError,
                            TimeoutError,
                            RuntimeError,
                            urllib.error.URLError,
                        ) as error:
                            warnings.append(f"turn {turn}: memory pass failed: {error}")
                    caller_text = ""
                    caller = response.get("caller")
                    caller_turn = 0
                    previous_operator_turn = None
                    rejected_operator_turn = None
                    rejection_error = ""
                    rejection_count = 0
                    transcript.clear()
                elif response.get("action") in {"Ask", "DirectoryCheck", "Tap"}:
                    caller_text = ""
                if not response.get("ok"):
                    rejection_error = response.get("error", "unknown error")
                    transcript.append(f"Backend rejected the proposed action: {rejection_error}")
                    rejected_operator_turn = operator_turn
                    rejection_count += 1
                    if used_rejection_fallback:
                        failures.append(
                            f"turn {turn}: backend rejected safe fallback: {rejection_error}"
                        )
                        break
                    continue
                rejected_operator_turn = None
                rejection_error = ""
                rejection_count = 0
            else:
                failures.append("maximum turn count reached before ending")
            if response["view"]["game_phase"] != "ended":
                failures.append("game did not reach an ending")
    finally:
        if backend.poll() is None:
            backend.terminate()
            try:
                backend.wait(timeout=5)
            except subprocess.TimeoutExpired:
                backend.kill()
                backend.wait(timeout=5)
        if llama is not None and llama.poll() is None:
            llama.terminate()
            try:
                llama.wait(timeout=10)
            except subprocess.TimeoutExpired:
                llama.kill()
                llama.wait(timeout=10)
    if assisted and args.require_autonomous:
        failures.append("ending required a correction or fallback action")
    status = "FAIL" if failures else "ASSISTED" if assisted else "PASS"
    print(f"{status} turns={spoken_turns} log={log_path}")
    for failure in failures:
        print(f"  - {failure}")
    for warning in warnings:
        print(f"  - warning: {warning}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
