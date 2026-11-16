# spaCy Integration

How AI Small Modell Coach integrates with spaCy 3.8+ (and stays forward
compatible with 3.9/4.x), plus the displaCy visualizer specification and
sanitization rules.

## Compatibility stance

- Target: spaCy >= 3.8 (tested against 3.8.x), Python >= 3.13.
- CI runs the smoke tests against the latest spaCy release monthly and records
  the result in a compatibility table at the bottom of this file.
- The logger reads the training-loop `info` dict defensively (see below), so a
  changed score shape degrades gracefully instead of crashing training.

The Rust side never imports spaCy. All spaCy work happens in
`python/coach_spacy` (pure Python; spaCy itself ships cp313 wheels).

## Training logger (`coach_spacy.logger`)

Two ways to enable:

1. Entry point (package `spacy_loggers`, name `coach.AimLogger.v1`).
2. Direct reference in a training config:

```toml
[training.logger]
@loggers = "coach_spacy.logger:coach_logger"
run_name = "textcat-baseline"
api_url = "http://127.0.0.1:8901"
# optional:
# session_id = "…"
# viz_path = "sample_docs.spacy"     # DocBin of sample docs (aim-spacy convention)
# model_log_interval = 100           # render viz every N steps
# viz_style = "ent"                  # ent | dep | span
# viz_n_docs = 3
# token = "…"                        # when COACH_TOKEN is set server-side
```

Protocol (identical shape to spaCy's built-in loggers):

```
coach_logger(**options) → setup(nlp, stdout, stderr) → (log_step(info), finalize())
```

### Forward-compatible `info` extraction

`coach_spacy/_compat.py` extracts values defensively:

- `score` scalar: `info["score"]`, falling back to `info["scores"]` (dict with
  a scalar `score` entry) — ready for future v3.9/4.x consolidation.
- `other_scores`: `info["other_scores"]`; nested dicts (tables) are skipped.
- `losses`, `epoch`, `step`, `checkpoints`, `seconds`, `words` — all optional.

On `setup`, the logger:
1. creates a run via coach-api (config = interpolated `nlp.config`, hparams,
   optional dataset hash/id from `COACH_DATASET_HASH` / `COACH_DATASET_ID`);
2. preloads up to `viz_n_docs` docs from `viz_path` (DocBin) for visualization;
3. chains the console logger so terminal output still works.

`log_step` POSTs metric points in batches to `POST /api/runs/{id}/metrics`;
`finalize` marks the run `completed`. Visualization runs every
`model_log_interval` steps and uploads displaCy renders as image artifacts;
failures there never abort training.

Dataset provenance for the run: set `COACH_DATASET_HASH` (sha256) and
optionally `COACH_DATASET_ID` in the training environment.

## displaCy visualizer (`python -m coach_spacy.render`)

A small FastAPI/uvicorn service that loads the pipeline once:

```bash
python -m coach_spacy.render --model en_core_web_sm --host 127.0.0.1 --port 8100
```

Endpoints:

- `GET /health` → `{"status": "ok", "model": …}`
- `POST /render` with `{text, style: "ent"|"dep"|"span", options, sentence_split}`
  → `{svg, html, meta}`

Per request: `doc = nlp(text)`; with `sentence_split`, each of
`[s.as_doc() for s in doc.sents]` is rendered separately and concatenated.
Rendering always uses `displacy.render(doc_or_docs, style=style, page=False,
minify=True, jupyter=False, options=options)`.

`displacy.serve` is **not** used anywhere; the browser talks to coach-api,
which proxies to this worker (`/api/visualize`), validates content type and
caps response size. PNG export (Visualize page and training-time renders) uses
pure-Python **cairosvg** — there is no chromium, html2image or headless
browser dependency anywhere in the stack (a deliberate fix of upstream
aim-spacy's eager `html2image` import).

### Options whitelist

Options are validated in `coach_spacy.render.OPTION_TYPES` before reaching
displacy (displacy interpolates option values into markup):

| key | type |
|---|---|
| compact, collapse_punct, collapse_phrases, fine_grained, add_lemma | bool |
| bg, color, font, title, spans_key | string (no `<`, `>`, control chars) |
| ents, spans | list of string/number |
| colors | dict string→string |
| distance, offset_x, arrow_spacing, arrow_width, arrow_stroke, word_spacing | int |

Unknown keys or wrong types return HTTP 422.

### Sanitization rules (three layers)

1. **Worker (this layer):** options whitelist + string validation (above).
2. **coach-api (Rust):** proxied `/api/visualize` caps response size,
   enforces `Content-Type: application/json`, and never lets the worker
   response through unbounded.
3. **Frontend:** displaCy markup embeds the raw input text. The Visualize page
   sanitizes the fragment with **DOMPurify** (svg + html profiles — needed for
   `dep` SVG output) before inlining. Scripts, inline event handlers and
   `javascript:` URLs are stripped even if they somehow survived layers 1–2.

### Attach to run / session

Visualizations can be saved as `Visualization` records (raw SVG text) from the
Visualize page, optionally linked to a run or session; during training the
logger instead uploads PNG renders as `Image` artifacts. Both surface in the
run detail view: the gallery shows them grouped by run/session.

## Smoke tests (CI)

`python/tests/smoke_test_training.py` — runs `spacy train` on a tiny 2-step
textcat job against a stub coach-api and asserts metric points arrive with
plausible names/shapes, plus `_compat` extraction across both possible score
shapes.

`python/tests/smoke_test_render.py` — boots the render worker with
`en_core_web_sm`, renders with every style, checks `ent` → entities HTML,
`dep` → SVG, `sentence_split` → per-sentence renders, and asserts no script
content or inline handlers in the markup; validates the options whitelist.

## Compatibility record

| date | spaCy | result |
|---|---|---|
| (scaffold) | 3.8.14 | smoke tests pass (py3.13.13) |

Update this table when the monthly CI job runs against the newest spaCy
release.
