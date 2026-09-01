#!/usr/bin/env python3
"""Serve a private, blinded TTS listening test on localhost."""

from __future__ import annotations

import argparse
import json
import mimetypes
import os
import random
import uuid
from datetime import datetime, timezone
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse


PROTOTYPE_DIR = Path(__file__).resolve().parent
DEFAULT_LINE_IDS = (
    "tts-01",
    "tts-03",
    "tts-05",
    "tts-06",
    "tts-07",
    "tts-09",
    "tts-11",
    "tts-12",
    "tts-13",
    "tts-14",
    "tts-19",
    "tts-20",
)
MAX_RATINGS_BYTES = 1024 * 1024


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


class RatingHandler(SimpleHTTPRequestHandler):
    server: "RatingServer"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(PROTOTYPE_DIR / "web"), **kwargs)

    def do_GET(self) -> None:  # noqa: N802
        route = urlparse(self.path).path
        if route == "/":
            self.path = "/tts_rating.html"
            super().do_GET()
            return
        if route == "/api/session":
            self._send_json(self.server.public_session)
            return
        if route == "/api/status":
            self._send_json(
                {
                    "complete": self.server.complete,
                    "result_path": str(self.server.result_path)
                    if self.server.complete
                    else None,
                }
            )
            return
        if route.startswith("/audio/"):
            self._send_audio(route)
            return
        self.send_error(HTTPStatus.NOT_FOUND)

    def do_POST(self) -> None:  # noqa: N802
        if urlparse(self.path).path != "/api/ratings":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            content_length = 0
        if not 0 < content_length <= MAX_RATINGS_BYTES:
            self._send_json(
                {"error": "Ratings payload is empty or too large"},
                HTTPStatus.BAD_REQUEST,
            )
            return
        try:
            payload = json.loads(self.rfile.read(content_length))
            self.server.save_ratings(payload)
        except (json.JSONDecodeError, ValueError) as error:
            self._send_json({"error": str(error)}, HTTPStatus.BAD_REQUEST)
            return
        self._send_json({"ok": True, "complete": self.server.complete})

    def _send_audio(self, route: str) -> None:
        parts = route.strip("/").split("/")
        if len(parts) != 3:
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        _, line_id, display_label = parts
        audio_path = self.server.audio_paths.get((line_id, display_label))
        if audio_path is None:
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        audio = audio_path.read_bytes()
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", mimetypes.guess_type(audio_path)[0] or "audio/wav")
        self.send_header("Content-Length", str(len(audio)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(audio)

    def _send_json(
        self, payload: object, status: HTTPStatus = HTTPStatus.OK
    ) -> None:
        encoded = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(encoded)


class RatingServer(ThreadingHTTPServer):
    def __init__(
        self,
        address: tuple[str, int],
        candidates: list[Path],
        results_dir: Path,
        selected_line_ids: tuple[str, ...],
        seed: int | None,
    ):
        super().__init__(address, RatingHandler)
        all_lines = json.loads((PROTOTYPE_DIR / "tts_lines.json").read_text())
        lines_by_id = {line["id"]: line for line in all_lines}
        missing_lines = [line_id for line_id in selected_line_ids if line_id not in lines_by_id]
        if missing_lines:
            raise ValueError(f"Unknown line IDs: {', '.join(missing_lines)}")

        self.results_dir = results_dir.resolve()
        self.results_dir.mkdir(parents=True, exist_ok=True)
        self.session_id = uuid.uuid4().hex[:12]
        self.complete = False
        self.result_path = self.results_dir / f"tts-blind-ratings-{self.session_id}.json"
        self.audio_paths: dict[tuple[str, str], Path] = {}
        randomizer = random.Random(seed)
        display_labels = [chr(ord("A") + index) for index in range(len(candidates))]
        mappings = {}
        public_lines = []

        for line_id in selected_line_ids:
            shuffled_candidates = candidates.copy()
            randomizer.shuffle(shuffled_candidates)
            mapping = dict(zip(display_labels, shuffled_candidates, strict=True))
            mappings[line_id] = {
                label: candidate.name for label, candidate in mapping.items()
            }
            for label, candidate in mapping.items():
                audio_path = (candidate / f"{line_id}.wav").resolve()
                if not audio_path.is_file():
                    raise ValueError(f"Missing listening sample: {audio_path}")
                self.audio_paths[(line_id, label)] = audio_path
            line = lines_by_id[line_id]
            public_lines.append(
                {
                    "id": line_id,
                    "category": line["category"],
                    "text": line["text"],
                    "samples": [
                        {"label": label, "url": f"/audio/{line_id}/{label}"}
                        for label in display_labels
                    ],
                }
            )

        self.public_session = {
            "session_id": self.session_id,
            "lines": public_lines,
            "score_scale": {
                "1": "unusable",
                "2": "poor",
                "3": "acceptable",
                "4": "good",
                "5": "excellent",
            },
        }
        self.private_session = {
            "session_id": self.session_id,
            "created_at": utc_now(),
            "candidate_directories": [str(path.resolve()) for path in candidates],
            "engine_mapping": mappings,
            "line_ids": list(selected_line_ids),
        }
        session_path = self.results_dir / f"tts-blind-session-{self.session_id}.json"
        session_path.write_text(json.dumps(self.private_session, indent=2) + "\n")
        os.chmod(session_path, 0o600)

    def save_ratings(self, payload: object) -> None:
        if not isinstance(payload, dict):
            raise ValueError("Ratings payload must be an object")
        if payload.get("session_id") != self.session_id:
            raise ValueError("Ratings session does not match")
        ratings = payload.get("ratings")
        if not isinstance(ratings, dict):
            raise ValueError("Ratings must be an object")
        known_lines = {line["id"] for line in self.public_session["lines"]}
        if not set(ratings).issubset(known_lines):
            raise ValueError("Ratings include an unknown line")
        status = payload.get("status", "draft")
        if status not in ("draft", "complete"):
            raise ValueError("Unknown ratings status")
        if status == "complete" and set(ratings) != known_lines:
            raise ValueError("Complete submission is missing one or more lines")

        result = {
            **self.private_session,
            "updated_at": utc_now(),
            "status": status,
            "ratings": ratings,
        }
        temporary_path = self.result_path.with_suffix(".tmp")
        temporary_path.write_text(json.dumps(result, indent=2) + "\n")
        os.chmod(temporary_path, 0o600)
        temporary_path.replace(self.result_path)
        self.complete = status == "complete"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8766)
    parser.add_argument("--samples-root", type=Path, required=True)
    parser.add_argument("--candidate", action="append", required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--seed", type=int)
    parser.add_argument("--line-id", action="append")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if len(args.candidate) < 2:
        raise SystemExit("Provide at least two --candidate directory names")
    if len(set(args.candidate)) != len(args.candidate):
        raise SystemExit("Candidate directory names must be unique")
    samples_root = args.samples_root.resolve()
    candidates = [(samples_root / candidate).resolve() for candidate in args.candidate]
    if any(path.parent != samples_root for path in candidates):
        raise SystemExit("Candidate names must be direct children of --samples-root")
    if any(not path.is_dir() for path in candidates):
        raise SystemExit("Every candidate must name an existing sample directory")

    server = RatingServer(
        (args.host, args.port),
        candidates,
        args.results_dir,
        tuple(args.line_id or DEFAULT_LINE_IDS),
        args.seed,
    )
    print(f"Blind TTS listening test: http://{args.host}:{args.port}")
    print(f"Private session/result directory: {server.results_dir}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nListening test stopped.")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
