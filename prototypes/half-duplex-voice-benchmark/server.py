#!/usr/bin/env python3
"""Local recording server for the throwaway voice benchmark prototype."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse


PROTOTYPE_DIR = Path(__file__).resolve().parent
PROMPTS = json.loads((PROTOTYPE_DIR / "prompts.json").read_text())
PROMPTS_BY_ID = {prompt["id"]: prompt for prompt in PROMPTS}
MAX_UPLOAD_BYTES = 25 * 1024 * 1024


class RecordingHandler(SimpleHTTPRequestHandler):
    server: "RecordingServer"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(PROTOTYPE_DIR / "web"), **kwargs)

    def do_GET(self) -> None:  # noqa: N802
        route = urlparse(self.path).path
        if route == "/api/prompts":
            self._send_json({"prompts": PROMPTS})
            return
        if route == "/api/status":
            completed = sorted(
                prompt_id
                for prompt_id in PROMPTS_BY_ID
                if any(self.server.recordings_dir.glob(f"{prompt_id}_*.wav"))
            )
            self._send_json({"completed": completed, "total": len(PROMPTS)})
            return
        super().do_GET()

    def do_POST(self) -> None:  # noqa: N802
        route = unquote(urlparse(self.path).path)
        prefix = "/api/recordings/"
        if not route.startswith(prefix):
            self.send_error(HTTPStatus.NOT_FOUND)
            return

        prompt_id = route.removeprefix(prefix)
        prompt = PROMPTS_BY_ID.get(prompt_id)
        if prompt is None:
            self._send_json({"error": "Unknown prompt"}, HTTPStatus.NOT_FOUND)
            return

        try:
            content_length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            content_length = 0
        if not 0 < content_length <= MAX_UPLOAD_BYTES:
            self._send_json(
                {"error": "Recording is empty or too large"},
                HTTPStatus.BAD_REQUEST,
            )
            return

        content_type = self.headers.get("Content-Type", "application/octet-stream")
        suffix = ".ogg" if "ogg" in content_type else ".webm"
        timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S_%fZ")
        stem = f"{prompt_id}_{timestamp}"
        raw_path = self.server.raw_dir / f"{stem}{suffix}"
        wav_path = self.server.recordings_dir / f"{stem}.wav"
        metadata_path = self.server.metadata_dir / f"{stem}.json"

        raw_path.write_bytes(self.rfile.read(content_length))
        conversion = subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                str(raw_path),
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_s16le",
                str(wav_path),
            ],
            capture_output=True,
            text=True,
        )
        if conversion.returncode != 0:
            self._send_json(
                {"error": "ffmpeg conversion failed", "detail": conversion.stderr},
                HTTPStatus.UNPROCESSABLE_ENTITY,
            )
            return

        metadata_path.write_text(
            json.dumps(
                {
                    "prompt_id": prompt_id,
                    "prompt": prompt["text"],
                    "condition": prompt["condition"],
                    "recorded_at": timestamp,
                    "browser_content_type": content_type,
                    "wav": str(wav_path.relative_to(self.server.data_dir)),
                    "raw": str(raw_path.relative_to(self.server.data_dir)),
                },
                indent=2,
            )
            + "\n"
        )
        self._send_json({"ok": True, "prompt_id": prompt_id, "wav": wav_path.name})

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


class RecordingServer(ThreadingHTTPServer):
    def __init__(self, address: tuple[str, int], data_dir: Path):
        super().__init__(address, RecordingHandler)
        self.data_dir = data_dir
        self.raw_dir = data_dir / "raw"
        self.recordings_dir = data_dir / "recordings"
        self.metadata_dir = data_dir / "metadata"
        for directory in (self.raw_dir, self.recordings_dir, self.metadata_dir):
            directory.mkdir(parents=True, exist_ok=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--data-dir", type=Path, default=PROTOTYPE_DIR / "data")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if shutil.which("ffmpeg") is None:
        raise SystemExit("ffmpeg is required but was not found on PATH")
    server = RecordingServer((args.host, args.port), args.data_dir.resolve())
    print(f"Voice benchmark recorder: http://{args.host}:{args.port}")
    print(f"Private recordings directory: {server.data_dir}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nRecorder stopped.")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
