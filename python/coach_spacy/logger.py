"""spaCy training logger for AI Small Modell Coach.

Clean-room implementation of the aim-spacy logger protocol for coach-api.

Usage in a spaCy training config::

    [training.logger]
    @loggers = "coach_spacy.logger:coach_logger"
    run_name = "textcat-baseline"
    api_url = "http://127.0.0.1:8901"

Also registered under the ``spacy_loggers`` entry point group as
``coach.AimLogger.v1``.
"""

from __future__ import annotations

import base64
import io
import sys
from datetime import datetime, timezone
from typing import IO, Any, Callable, Dict, List, Optional, Tuple, Union

from spacy import displacy
from spacy import Language
from spacy.training.loggers import console_logger

from ._compat import extract_context, extract_score, flatten_metrics
from .api import CoachApiClient

LogStep = Callable[[Optional[Dict[str, Any]]], None]
Finalize = Callable[[], None]


def _utcnow() -> str:
    return datetime.now(timezone.utc).isoformat()


def coach_logger(
    run_name: Optional[str] = None,
    api_url: str = "http://127.0.0.1:8901",
    session_id: Optional[str] = None,
    token: Optional[str] = None,
    viz_path: Optional[str] = None,
    model_log_interval: Optional[int] = None,
    viz_style: str = "ent",
    viz_n_docs: int = 3,
):
    """Logger factory for spaCy's training logger protocol.

    Returns ``setup(nlp, stdout, stderr) -> (log_step, finalize)``.
    """

    def setup(
        nlp: Language, stdout: IO = sys.stdout, stderr: IO = sys.stderr
    ) -> Tuple[LogStep, Finalize]:
        nonlocal run_name, viz_path, model_log_interval, viz_style, viz_n_docs

        client = CoachApiClient(api_url, token=token)
        config = nlp.config.interpolate() if hasattr(nlp.config, "interpolate") else None

        run_name = run_name or f"spacy-train-{nlp.meta.get('lang', 'xx')}"
        hparams: Dict[str, Any] = {}
        try:
            batch_size = nlp.config["training"]["batch_size"]
            hparams["batch_size"] = str(batch_size) if not isinstance(batch_size, (int, float)) else batch_size
        except (KeyError, TypeError):
            pass
        for key in ("seed", "gpu_allocator"):
            value = nlp.config.get("training", {}).get(key)
            if value is not None:
                hparams[key] = value
        dataset_hash = None
        try:
            # Coach convention: dataset provenance via COACH_DATASET_HASH env.
            import os

            dataset_hash = os.environ.get("COACH_DATASET_HASH")
            dataset_id = os.environ.get("COACH_DATASET_ID")
        except Exception:
            dataset_id = None

        run = client.create_run(
            name=run_name,
            session_id=session_id,
            config=config,
            hparams=hparams,
            tags=["spacy", nlp.meta.get("version", "")] if nlp.meta.get("version") else ["spacy"],
        )
        run_id: str = run["id"]

        # Sample docs for training-time visualization (same viz_path
        # convention as aim-spacy: a .spacy DocBin of sample documents).
        viz_docs = []
        if viz_path and model_log_interval:
            viz_docs = _load_viz_docs(nlp, viz_path, viz_n_docs)

        console_log_step, console_finalize = console_logger(progress_bar=False)(
            nlp, stdout, stderr
        )

        def log_step(info: Optional[Dict[str, Any]]) -> None:
            console_log_step(info)
            if not info:
                return
            epoch = info.get("epoch")
            step = info.get("step", 0)
            score, _ = extract_score(info)

            points: List[Dict[str, Any]] = []
            if score is not None:
                points.append(
                    _point(run_id, "score", score, step, epoch, {"type": "score"})
                )
            for name, value, context in flatten_metrics(info):
                if name == "score":
                    continue
                points.append(_point(run_id, name, value, step, epoch, context))

            for key, value in extract_context(info).items():
                if key in ("epoch", "step"):
                    continue
                points.append(
                    _point(run_id, f"__meta__.{key}", float(value), step, epoch, {"type": "meta"})
                )

            if points:
                client.append_metrics(run_id, points)

            if viz_docs and model_log_interval and step and step % model_log_interval == 0:
                try:
                    _track_visualizations(
                        client, run_id, nlp, viz_docs, step, epoch, viz_style
                    )
                except Exception as err:  # visualization must never kill training
                    print(f"coach_spacy: visualization failed: {err}", file=stderr)

        def finalize() -> None:
            console_finalize()
            try:
                client.mark_run_status(run_id, "completed")
            except Exception:
                pass

        return log_step, finalize

    return setup


def _point(
    run_id: str, name: str, value: float, step: int, epoch, context: Dict[str, Any]
) -> Dict[str, Any]:
    out: Dict[str, Any] = {
        "run_id": run_id,
        "name": name,
        "context": context,
        "step": int(step or 0),
        "value": float(value),
        "timestamp": _utcnow(),
    }
    if epoch is not None:
        out["epoch"] = int(epoch)
    return out


def _load_viz_docs(nlp: Language, viz_path: str, n_docs: int) -> List[Any]:
    from spacy.tokens import DocBin

    docbin = DocBin().from_disk(viz_path)
    # Render the raw stored docs (their entities already live on the tokens).
    return list(docbin.get_docs(nlp.make_doc("").vocab))[:n_docs]


def _track_visualizations(
    client: CoachApiClient,
    run_id: str,
    nlp: Language,
    docs: List[Any],
    step: int,
    epoch: Optional[int],
    style: str,
) -> None:
    """Render sample docs with displaCy and upload as image artifacts.

    SVG is stored raw; PNG via pure-Python cairosvg when available (never
    chromium/html2image). Failures are non-fatal.
    """
    markup = displacy.render(
        docs, style=style, page=False, minify=True, jupyter=False
    )
    name = f"viz-{style}-step-{step}"
    meta = {"style": style, "format": "svg", "step": step, "epoch": epoch}
    try:
        png_base64 = _svg_to_png_base64(markup)
    except Exception:
        # SVG-only path: coach-api stores the PNG field for consistency; use
        # the raw SVG as a hint payload for the gallery.
        png_base64 = base64.b64encode(markup.encode()).decode()
        meta["format"] = "svg-as-png-slot"
    client.put_image(
        run_id,
        name=name,
        png_base64=png_base64,
        caption=f"displaCy {style} render at step {step}",
        step=step,
        epoch=epoch,
        meta=meta,
    )


def _svg_to_png_base64(markup: str) -> str:
    try:
        import cairosvg
    except ImportError as err:  # optional extra: python -m pip install coach-spacy[viz]
        raise RuntimeError(
            "PNG export requires cairosvg (pip install coach-spacy[viz])"
        ) from err
    png_bytes = cairosvg.svg2png(bytestring=markup.encode())
    return base64.b64encode(png_bytes).decode()


def _register_direct_reference() -> None:
    """Also make `@loggers = "coach_spacy.logger:coach_logger"` work: spaCy
    resolves that string as a registry name, so register the factory under
    the literal dotted name at import time."""
    try:
        from spacy.util import registry

        registry.loggers("coach_spacy.logger:coach_logger", func=coach_logger)
    except Exception:
        # Entry-point usage (`coach.AimLogger.v1`) still works if spaCy's
        # registry layout changes.
        pass


_register_direct_reference()
