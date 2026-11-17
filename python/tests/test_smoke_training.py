"""Smoke test (a): spaCy `train` with the coach logger against a stub server.

Runs a tiny textcat training for 2 steps with the
`coach_spacy.logger:coach_logger` and asserts metric points arrive at the
stub coach-api.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest
import spacy
from spacy.cli.init_config import init_config
from spacy.tokens import DocBin

TEXTS = [
    ("The refund was processed quickly.", {"cats": {"POSITIVE": 1.0, "NEGATIVE": 0.0}}),
    ("I am frustrated with the service.", {"cats": {"POSITIVE": 0.0, "NEGATIVE": 1.0}}),
    ("Great experience overall.", {"cats": {"POSITIVE": 1.0, "NEGATIVE": 0.0}}),
    ("The package arrived broken.", {"cats": {"POSITIVE": 0.0, "NEGATIVE": 1.0}}),
    ("Support was helpful and kind.", {"cats": {"POSITIVE": 1.0, "NEGATIVE": 0.0}}),
    ("This is a terrible product.", {"cats": {"POSITIVE": 0.0, "NEGATIVE": 1.0}}),
    ("Everything worked as expected.", {"cats": {"POSITIVE": 1.0, "NEGATIVE": 0.0}}),
    ("Worst purchase of the year.", {"cats": {"POSITIVE": 0.0, "NEGATIVE": 1.0}}),
]

LABELS = ["POSITIVE", "NEGATIVE"]


def write_docbin(path: Path, lang: str) -> None:
    nlp = spacy.blank(lang)
    docbin = DocBin(store_user_data=False)
    for text, ann in TEXTS:
        doc = nlp.make_doc(text)
        doc.cats = dict(ann["cats"])
        docbin.add(doc)
    docbin.to_disk(path)


def build_config(tmp_path: Path, stub_url: str):
    config = init_config(lang="en", pipeline=["textcat"], optimize="efficiency")
    train_path = tmp_path / "train.spacy"
    dev_path = tmp_path / "dev.spacy"
    write_docbin(train_path, "en")
    write_docbin(dev_path, "en")

    training = config["training"]
    training["max_steps"] = 2
    training["patience"] = -1
    training["eval_frequency"] = 100000  # skip in-training eval entirely
    training["logger"] = {
        "@loggers": "coach.Logger.v1",
        "run_name": "smoke-textcat",
        "api_url": stub_url,
    }
    return config, train_path, dev_path


@pytest.mark.slow
def test_direct_reference_resolves_in_process(stub_coach_api):
    # `@loggers = "coach_spacy.logger:coach_logger"` works once coach_spacy is
    # imported (the module registers the dotted name on import). Fresh CLI
    # subprocesses should use the entry-point name.
    from spacy.util import registry

    import coach_spacy.logger  # noqa: F401  (performs the registration)

    resolved = registry.resolve(
        {
            "logger": {
                "@loggers": "coach_spacy.logger:coach_logger",
                "run_name": "direct-ref",
                "api_url": stub_coach_api.url,
            }
        }
    )["logger"]
    assert callable(resolved)


@pytest.mark.slow
def test_training_metrics_reach_stub_server(tmp_path, stub_coach_api):
    config, train_path, dev_path = build_config(tmp_path, stub_coach_api.url)
    cfg_path = tmp_path / "config.cfg"
    config.to_disk(cfg_path)

    # Go through the CLI (same as real usage); default --gpu-id -1 = CPU, so
    # no CuPy/torch requirement in CI.
    env = dict(os.environ)
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "spacy",
            "train",
            str(cfg_path),
            "--output",
            str(tmp_path / "out"),
            "--paths.train",
            str(train_path),
            "--paths.dev",
            str(dev_path),
        ],
        capture_output=True,
        text=True,
        env=env,
        timeout=300,
    )
    assert result.returncode == 0, f"spacy train failed:\n{result.stdout[-3000:]}\n{result.stderr[-3000:]}"

    record = stub_coach_api.record
    # The logger created exactly one run with our interpolated config.
    assert len(record["runs"]) >= 1
    run = record["runs"][0]
    assert run["name"] == "smoke-textcat"
    assert "config" in run and run["config"]

    # Metric points arrived with plausible step numbering and shapes.
    steps = {p["step"] for p in record["metric_points"]}
    assert steps, "no metric points reached the stub server"
    assert steps <= {0, 1, 2}
    names = {p["name"] for p in record["metric_points"]}
    # spaCy names component losses after the component itself.
    assert "textcat" in names or "textcat_loss" in names
    assert "score" in names
    assert "cats_score" in names
    for point in record["metric_points"]:
        assert set(point) >= {"name", "step", "value", "timestamp"}
        assert isinstance(point["value"], (int, float))

    # The logger finalizes the run.
    assert record["status_updates"], "finalize did not mark the run"


@pytest.mark.slow
def test_score_extraction_forward_compatible():
    from coach_spacy._compat import extract_score, flatten_metrics

    # v3.8 shape
    score, other = extract_score(
        {"score": 0.91, "other_scores": {"cats_score": 0.91}, "losses": {"textcat": 1.5}}
    )
    assert score == pytest.approx(0.91)
    assert other == {"cats_score": 0.91}

    # hypothetical future shape: single "scores" dict
    score, other = extract_score({"scores": {"score": 0.8, "cats_score_f": 0.7}})
    assert score == pytest.approx(0.8)
    assert other["cats_score_f"] == pytest.approx(0.7)

    # flattened metric triples skip nested dicts and non-numeric junk
    triples = flatten_metrics(
        {"score": 0.5, "other_scores": {"cats_score": 0.6, "table": {"a": 1}}}
    )
    assert ("cats_score", 0.6, {"type": "other_scores"}) in triples
