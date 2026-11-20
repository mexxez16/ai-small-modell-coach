# AI Small Modell Coach

A self-hosted coach for fine-tuning small NLP models (starting with spaCy) —
an experiment tracker, a training-data curation workbench, and a displaCy
visualizer in a single app, driven from a built-in AI chat and terminal.

<p align="center">
  <img src="images/AI%20Small%20Language%20Trainer%20Title%20Image.webp"
       alt="AI Small Language Trainer" />
</p>
<p align="center"><em>Currently spaCy only</em></p>

[![CI](https://github.com/mexxez16/ai-small-modell-coach/actions/workflows/ci.yml/badge.svg)](https://github.com/mexxez16/ai-small-modell-coach/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## Features

### Experiment tracking

- **Runs & metrics** — create runs from the UI or log them straight from your
  spaCy training loop; metrics stream in over WebSocket and land in an
  embedded SQLite database (WAL mode, incremental writes, no external server).
- **Run query engine** — filter and compare runs with a query AST
  (`/api/query`), group runs into **sessions** and compare them.
- **Images** — attach example images/metadata to runs and browse them.

### Training-data curation

- **Data sessions & data runs** — training-data sessions group pipeline runs;
  each run executes a configurable, multi-step curation pipeline
  (import → annotate → review → export).
- **Configurable pipeline editor** — six sections, per-step config files,
  model override for AI annotation, prompt templates with example types.
- **Review modes** — review curated items as cards, as a paginated bulk
  table, or both; approve/reject per label, with a full audit history per
  item (`/api/items/{id}/history`).
- **Datasets & export** — versioned datasets with export endpoints
  (`/api/datasets/{id}/export`, JSONL/DocBin-oriented).
- **AI annotation** — model candidates (`/api/model-candidates`) and
  annotation steps that suggest items, which only become data through the
  review workflow.

### Visualization & workflow

- **displaCy in the browser** — `ent`, `dep` and `span` styles rendered by a
  Python render worker (`coach_spacy.render`, FastAPI/uvicorn + cairosvg),
  exported as SVG/PNG. No chromium, no `displacy.serve`.
- **Chat + terminal docks** — an embedded Mistral Vibe chat window
  (OpenAI-compatible streaming providers + vibe PTY) and an embedded PTY
  terminal (`/ws/terminal`), so the whole workflow stays in the browser.
- **Command palette, file-system browser, settings** — provider management
  with connection tests, per-step pipeline configs.
- **Typed frontend client** — generated from the served OpenAPI spec
  (`/api/openapi.json`).

## Architecture

```
┌───────────────────────────────── apps/web (React 18 + TS + Vite) ────────┐
│  Dashboard  Sessions  Runs  Training Data  Visualize  Images  Settings   │
│  Right dock: Chat (collapsible) · Bottom dock: Terminal (collapsible)    │
└─────────────────────────────▲────────────────────────────────────────────┘
                REST + WS     │  (typed client, generated from OpenAPI)
┌─────────────────────────────┴────────────────────────────────────────────┐
│ crates/coach-api (axum): REST, /ws/* fan-out, PTY mgmt, chat providers,  │
│ render-worker process mgmt, static file serving (SPA fallback)           │
└───────▲─────────────────────────────────────┬────────────────────────────┘
        │ shared traits                       │ process mgmt
┌───────┴───────────────┐         ┌───────────▼─────────────┐
│ crates/coach-core     │         │ python render worker    │
│ domain model          │         │ (coach_spacy.render,    │
│ storage traits        │         │  FastAPI/uvicorn, spaCy)│
│ Run query AST         │         └─────────────────────────┘
│ SQLite (WAL)          │
└───────▲───────────────┘
        │ abi3 wheel (PyO3/maturin)
┌───────┴───────────────┐
│ crates/coach-pyo3     │  → python/coach_spacy (logger + glue)
│ coach ingest SDK      │
└───────────────────────┘
```

Why this shape:

- **One embedded SQLite database** (`<COACH_DATA_DIR>/coach.db`, bundled
  rusqlite): no RocksDB, no external database server, so new Python
  releases can never be blocked by a native dependency. Legacy per-file JSON
  snapshots migrate automatically on first boot.
- **The Python layer is tiny by design** — only the spaCy logger (registered
  as a spaCy training logger entry point) and the displaCy render worker.
- **The SDK wheel is `abi3-py313`** (PyO3/maturin), so it runs on 3.13+
  interpreters without rebuilds.

## Repository layout

```
ai-small-modell-coach/
├── apps/web/          # React 18 + TypeScript + Vite frontend (own design system)
├── crates/
│   ├── coach-core/    # Rust: domain model + storage/query engine traits (SQLite backend)
│   ├── coach-api/     # Rust: axum HTTP/WebSocket server, PTY, render-worker mgmt
│   └── coach-pyo3/    # Rust -> Python SDK (PyO3/maturin, abi3-py313)
├── python/coach_spacy # The only Python layer: spaCy logger + displaCy render worker
├── docs/              # ARCHITECTURE, DATA_MODEL, SPACY_INTEGRATION
├── scripts/           # dev setup + codegen
└── .github/workflows/ # CI (Rust build, py3.13 wheel + smoke tests, frontend build)
```

## Quickstart

Prerequisites: Rust (stable), Python >= 3.13, Node >= 18, and a local Python
environment with spaCy >= 3.8. `make dev-setup` bootstraps the rest.

```bash
# 1. Bootstrap .venv (py3.13, spaCy) + npm install
make dev-setup

# 2. Build the Rust workspace
make build

# 3. Build the Python SDK wheel (maturin, abi3-py313)
make wheel

# 4. Build the frontend
make web-build

# 5. Run the API server (serves the built frontend too)
#    Default port is 8901; if it is taken, the server scans upward
#    (8901, 8902, ...). Set COACH_PORT=<n> to pin an exact port.
make serve

# 6. Run tests (Rust unit tests, Python smoke tests, frontend typecheck)
make test
```

## Usage

Logging a spaCy training run with the coach logger:

```toml
# in your spaCy training config
[training.logger]
@loggers = "coach_spacy.logger:coach_logger"
api_url = "http://127.0.0.1:8901"
run_name = "textcat-baseline"
```

Launching the displaCy render worker:

```bash
python -m coach_spacy.render --model en_core_web_sm --host 127.0.0.1 --port 8100
```

All REST endpoints are documented in the served OpenAPI spec at
`http://127.0.0.1:8901/api/openapi.json`; live metric/chat/job streams run
over `/ws/metrics`, `/ws/chat`, `/ws/jobs` and `/ws/terminal`.

## Compatibility targets

- Python >= 3.13 (the SDK wheel is `abi3-py313`, so it also runs on future
  3.13+ interpreters without rebuilds).
- spaCy >= 3.8, designed so the logger keys defensively on score/scores
  shapes and tolerates 3.9/4.x changes (see `docs/SPACY_INTEGRATION.md`).
  A monthly CI job probes spaCy compatibility.
- No chromium anywhere: displaCy PNG export uses pure-Python cairosvg, and
  `displacy.serve` is never used — all rendering goes through the render
  worker.

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system design and decisions
- [`docs/DATA_MODEL.md`](docs/DATA_MODEL.md) — entities and persistence
- [`docs/SPACY_INTEGRATION.md`](docs/SPACY_INTEGRATION.md) — logger and
  visualizer integration details
- [`python/coach_spacy/README.md`](python/coach_spacy/README.md) — SDK usage

## License

MIT — see [`LICENSE`](LICENSE).
