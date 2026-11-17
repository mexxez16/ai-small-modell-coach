"""Smoke test (b): start the render worker with en_core_web_sm, POST a
sentence with style="ent", and assert sanitized-compliant markup comes back.

Sanitization itself lives in the frontend (DOMPurify) and on coach-api
(response-size caps); the worker contract under test here is: valid options
whitelist, no script/event-handler content in markup, correct SVG vs HTML
output per style.
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from coach_spacy.render import validate_options  # noqa: E402

SENTENCE = "Apple is looking at buying U.K. startup for $1 billion."


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


@pytest.fixture(scope="module")
def render_worker():
    port = free_port()
    proc = subprocess.Popen(
        [
            sys.executable,
            "-m",
            "coach_spacy.render",
            "--model",
            "en_core_web_sm",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    base = f"http://127.0.0.1:{port}"
    deadline = time.time() + 60
    ready = False
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"{base}/health", timeout=2) as resp:
                if resp.status == 200:
                    ready = True
                    break
        except Exception:
            if proc.poll() is not None:
                out = proc.stdout.read().decode(errors="replace")
                raise RuntimeError(f"worker died during startup:\n{out}")
            time.sleep(0.5)
    if not ready:
        proc.kill()
        raise RuntimeError("render worker did not become healthy in time")
    yield base
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()


def post(base: str, payload: dict) -> tuple[int, dict]:
    req = urllib.request.Request(
        f"{base}/render",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, json.loads(err.read() or b"{}")


@pytest.mark.slow
def test_ent_render_returns_clean_html(render_worker):
    status, payload = post(
        render_worker,
        {"text": SENTENCE, "style": "ent", "options": {}},
    )
    assert status == 200, payload
    markup = payload["html"]
    assert 'class="entities"' in markup
    assert payload["meta"]["style"] == "ent"
    # Hard rules: never embed scripts or inline event handlers.
    assert "<script" not in markup.lower()
    assert "onerror=" not in markup.lower()
    assert "javascript:" not in markup.lower()
    # Entity spans from en_core_web_sm exist for this sentence.
    assert 'class="ent' in markup


@pytest.mark.slow
def test_dep_render_returns_svg(render_worker):
    status, payload = post(
        render_worker,
        {"text": SENTENCE, "style": "dep", "options": {"distance": 100}},
    )
    assert status == 200, payload
    assert payload["svg"].lstrip().startswith("<svg")
    assert "<script" not in payload["svg"].lower()
    assert payload["meta"]["style"] == "dep"


@pytest.mark.slow
def test_sentence_split_renders_each_sentence(render_worker):
    text = "Dr. Smith loves tea. He drinks it daily."
    status, payload = post(
        render_worker,
        {"text": text, "style": "ent", "options": {}, "sentence_split": True},
    )
    assert status == 200, payload
    assert payload["meta"]["num_rendered"] == 2


def test_options_whitelist_rejects_injection():
    with pytest.raises(ValueError):
        validate_options({"evil_key": "<script>alert(1)</script>"})
    with pytest.raises(ValueError):
        validate_options({"ents": ["ok", (1, 2)]})  # nested tuples rejected
    with pytest.raises(ValueError):
        validate_options({"bg": 'red"><script>'})  # angle brackets in strings
    assert validate_options({"distance": 120, "compact": True}) == {
        "distance": 120,
        "compact": True,
    }


def test_convert_docbin_roundtrip(render_worker):
    """DocBin export: text items -> .spacy bytes -> reload with spaCy."""
    items = [
        {"text": "The cat sat on the mat.", "label": "neutral", "split": "train"},
        {"text": "I absolutely love this!", "label": "positive", "split": "train"},
        {"text": "Terrible experience.", "label": "negative", "split": "dev"},
        {"text": "", "label": "broken", "split": "train"},  # skipped
    ]
    status, payload = _post_docbin(render_worker, items)
    assert status == 200, payload
    assert payload["stats"]["counts"]["train"] == 2
    assert payload["stats"]["counts"]["dev"] == 1
    assert len(payload["stats"]["skipped"]) == 1

    # Reload the returned DocBin with spaCy and check label supervision.
    import base64

    from spacy.tokens import DocBin

    docbin = DocBin().from_bytes(base64.b64decode(payload["docbin_b64"]))
    nlp = __import__("spacy").blank("en")
    docs = list(docbin.get_docs(nlp.vocab))
    assert len(docs) == 3
    cats = [set(doc.cats) for doc in docs]
    assert {"neutral"} in cats
    assert any("negative" in c for c in cats)


def _post_docbin(base: str, items: list) -> tuple[int, dict]:
    req = urllib.request.Request(
        f"{base}/convert_docbin",
        data=json.dumps({"items": items}).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, json.loads(err.read() or b"{}")
