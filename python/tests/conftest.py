"""Shared fixtures for coach_spacy smoke tests.

CI (and professional setups) run `pip install -e python` first; the sys.path
fallback below also lets the tests run straight from a source checkout.
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

PY_ROOT = Path(__file__).resolve().parents[1]
if str(PY_ROOT) not in sys.path:
    sys.path.insert(0, str(PY_ROOT))


class RecordingHandler(BaseHTTPRequestHandler):
    """Stub of coach-api: records the writes the logger performs."""

    server_ref = None  # set by the fixture

    def log_message(self, *args):  # silence request logging
        pass

    def _read_json(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b"{}"
        return json.loads(raw or b"{}")

    def _respond(self, payload, status=200):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        record = self.server.record
        payload = self._read_json()
        if self.path == "/api/runs":
            record["runs"].append(payload)
            self._respond({"id": "stub-run-1", "name": payload.get("name", "")})
        elif self.path == "/api/runs/stub-run-1/metrics":
            record["metric_points"].extend(payload if isinstance(payload, list) else [payload])
            self._respond({"accepted": len(payload) if isinstance(payload, list) else 1})
        elif self.path == "/api/runs/stub-run-1/images":
            record["images"].append(payload)
            self._respond({"id": "stub-image-1"})
        elif self.path == "/api/runs/stub-run-1/status":
            record["status_updates"].append(payload)
            self._respond({"id": "stub-run-1", "status": payload.get("status")})
        else:
            self._respond({"error": f"untracked path {self.path}"}, status=404)

    def do_GET(self):
        if self.path == "/api/health":
            self._respond({"status": "ok", "version": "stub"})
        else:
            self._respond({"error": "not found"}, status=404)


@pytest.fixture()
def stub_coach_api():
    """Start a recording stub of coach-api on a random local port."""
    record = {"runs": [], "metric_points": [], "images": [], "status_updates": []}

    class Server(ThreadingHTTPServer):
        daemon_threads = True

    server = Server(("127.0.0.1", 0), RecordingHandler)
    server.record = record
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    class Stub:
        def __init__(self, srv):
            self._srv = srv
            self.record = record

        @property
        def url(self) -> str:
            host, port = self._srv.server_address
            return f"http://{host}:{port}"

        def stop(self):
            self._srv.shutdown()
            self._srv.server_close()
            thread.join(timeout=5)

    stub = Stub(server)
    yield stub
    stub.stop()
