"""displaCy render worker: python -m coach_spacy.render --model en_core_web_sm

A small FastAPI/uvicorn service. Loads the spaCy pipeline once, then renders
per request via spacy.displacy. The Rust API (coach-api) manages this process
and proxies /api/visualize to it; the Rust side never imports spaCy.

Security notes:
- displacy options are validated against a whitelist of keys/types before
  being handed to displacy (they get embedded into the markup).
- Rendering IS the source of markup; downstream, coach-api caps response
  sizes and the frontend sanitizes HTML/SVG with DOMPurify before inlining.
"""

from __future__ import annotations

import argparse
import base64
import io
import re
from typing import Any, Dict, List, Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field
from spacy import displacy

# Whitelist of displacy options: allowed key -> allowed python type(s).
OPTION_TYPES: Dict[str, tuple] = {
    # shared
    "compact": (bool,),
    "bg": (str,),
    "color": (str,),
    "font": (str,),
    "title": (str,),
    # ent / span
    "ents": (list,),
    "colors": (dict,),
    "spans_key": (str,),
    "spans": (list,),
    # dep
    "distance": (int,),
    "offset_x": (int,),
    "arrow_spacing": (int,),
    "arrow_width": (int,),
    "arrow_stroke": (int,),
    "word_spacing": (int,),
    "collapse_punct": (bool,),
    "collapse_phrases": (bool,),
    "fine_grained": (bool,),
    "add_lemma": (bool,),
}

TEXT_LIMIT = 20_000


class RenderRequest(BaseModel):
    text: str = Field(min_length=1, max_length=TEXT_LIMIT)
    # "ent" | "dep" | "span"
    style: str = Field(pattern="^(ent|dep|span)$")
    options: Dict[str, Any] = Field(default_factory=dict)
    sentence_split: bool = False


class RenderResponse(BaseModel):
    svg: str
    html: str
    meta: Dict[str, Any]


class PngResponse(BaseModel):
    png_base64: str
    meta: Dict[str, Any]


def validate_options(options: Dict[str, Any]) -> Dict[str, Any]:
    """Validate displacy options against the whitelist; raise ValueError."""
    clean: Dict[str, Any] = {}
    for key, value in (options or {}).items():
        if key not in OPTION_TYPES:
            raise ValueError(f"displacy option not allowed: {key!r}")
        expected = OPTION_TYPES[key]
        if not isinstance(value, expected):
            raise ValueError(
                f"displacy option {key!r} must be one of "
                f"{[t.__name__ for t in expected]}, got {type(value).__name__}"
            )
        if isinstance(value, dict):
            # colors: str -> str only
            if not all(
                isinstance(k, str) and isinstance(v, str) for k, v in value.items()
            ):
                raise ValueError("dict options must map str to str")
        if isinstance(value, list):
            if not all(isinstance(v, (str, int, float)) for v in value):
                raise ValueError("list options must contain str/int/float only")
        if isinstance(value, str):
            if not _safe_str(value):
                raise ValueError(f"option {key!r} contains disallowed characters")
        clean[key] = value
    return clean


_CONTROL_RE = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f]")

# Control characters and < > in option strings (bg/color/title/font) would end
# up inside attribute values in the markup; displaCy html-escapes some of it,
# but reject pathological input at the boundary in addition to frontend
# sanitization.
_DISALLOWED_STR_RE = re.compile(r"[<>]")


def _safe_str(value: str) -> bool:
    return not _CONTROL_RE.search(value) and not _DISALLOWED_STR_RE.search(value)


def create_app(model: str) -> FastAPI:
    import spacy

    try:
        nlp = spacy.load(model)
    except Exception as err:
        raise RuntimeError(
            f"cannot load spaCy pipeline {model!r}: {err}"
        ) from err

    app = FastAPI(title="coach_spacy render worker")
    state = {"model": model, "nlp": nlp}

    @app.get("/health")
    def health() -> Dict[str, Any]:
        return {"status": "ok", "model": state["model"]}

    @app.post("/render", response_model=RenderResponse)
    def render(req: RenderRequest) -> RenderResponse:
        try:
            options = validate_options(req.options)
        except ValueError as err:
            raise HTTPException(status_code=422, detail=str(err)) from err

        nlp_current: Any = state["nlp"]
        doc = nlp_current(req.text)

        pieces: List[str] = []
        n_rendered = 0
        if req.sentence_split:
            for sent in doc.sents:
                markup = displacy.render(
                    sent.as_doc(),
                    style=req.style,
                    page=False,
                    minify=True,
                    jupyter=False,
                    options=options,
                )
                pieces.append(markup)
                n_rendered += 1
        else:
            pieces.append(
                displacy.render(
                    doc,
                    style=req.style,
                    page=False,
                    minify=True,
                    jupyter=False,
                    options=options,
                )
            )
            n_rendered = 1

        return RenderResponse(
            svg="".join(pieces),
            html="".join(pieces),
            meta={
                "model": state["model"],
                "style": req.style,
                "sentence_split": req.sentence_split,
                "num_rendered": n_rendered,
                "n_chars": len(req.text),
                "n_tokens": len(doc),
                "options": options,
            },
        )

    @app.post("/render_png", response_model=PngResponse)
    def render_png(req: RenderRequest) -> PngResponse:
        """SVG -> PNG conversion for raster export.

        Only `dep` renders are pure SVG; `ent`/`span` return HTML fragments
        that no pure-Python rasterizer can convert (no headless browser by
        design), so those are rejected with a 422.
        """
        if req.style != "dep":
            raise HTTPException(
                status_code=422,
                detail="PNG export requires style='dep' (ent/span render HTML, not SVG)",
            )
        rendered = render(req)
        try:
            import io

            import cairosvg
        except ImportError as err:
            raise HTTPException(
                status_code=503,
                detail="cairosvg not installed; install the package with the 'viz' extra",
            ) from err
        png_bytes = cairosvg.svg2png(
            bytestring=rendered.svg.encode("utf-8"),
            background_color=req.options.get("bg", "#ffffff"),
        )
        meta = dict(rendered.meta)
        return PngResponse(
            png_base64=base64.b64encode(png_bytes).decode("ascii"), meta=meta
        )

    return app


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="python -m coach_spacy.render",
        description="displaCy render worker for AI Small Modell Coach",
    )
    parser.add_argument("--model", required=True, help="spaCy pipeline path or name")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0, help="0 = pick a free port")
    args = parser.parse_args()

    import uvicorn

    app = create_app(args.model)
    port = args.port
    if port == 0:
        import socket

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind((args.host, 0))
            port = sock.getsockname()[1]
        print(f"render worker port: {port}", flush=True)
    uvicorn.run(app, host=args.host, port=port, log_level="warning")


if __name__ == "__main__":
    main()
