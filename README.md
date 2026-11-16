# AI Small Modell Coach

A self-hosted coach for fine-tuning small NLP models (starting with spaCy):

- **Track training runs** like Aim — metrics, hyperparameters, run comparison —
  with a Rust backend backed by Parquet files and embedded DuckDB (no RocksDB,
  no external database).
- **Visualize Docs with displaCy** inside the app: `ent`, `dep` and `span`
  styling, live in the browser, exported as SVG/PNG.
- **Curate training data** like a dataset workbench: review states, audit
  trail, dataset versioning, and an SSOT JSON to seed JSONL to DocBin
  conversion pipeline.
- **Group runs into sessions** and compare them.
- **Drive the whole workflow from a chat + terminal** — an embedded Mistral
  Vibe chat window and an embedded terminal — without leaving the browser.

License: MIT (see `LICENSE` and `NOTICE`).

The upstream `aimhubio/aim` and `aimhubio/aim-spacy` repositories are used as
**look-only references** for scope and UX; no code is derived from them (see
`NOTICE`). Reference checkouts live in `Example Repos/` in the parent
directory of this repository and are not part of this codebase.

## Repository layout

```
ai-small-modell-coach/
├── apps/web/          # React 18 + TypeScript + Vite frontend (own design system)
├── crates/
│   ├── coach-core/    # Rust: domain model + storage/query engine traits (Parquet/DuckDB)
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
# 1. Build the Rust workspace
make build

# 2. Build the Python SDK wheel (maturin, abi3-py313)
make wheel

# 3. Build the frontend
make web-build

# 4. Run the API server (serves the built frontend too)
make serve

# 5. Run tests (Rust unit tests, Python smoke tests, frontend lint/build)
make test
```

Training a spaCy pipeline with the coach logger:

```toml
# in your spaCy training config
[training.logger]
@loggers = "coach_spacy.logger:coach_logger"
api_url = "http://127.0.0.1:8000"
run_name = "textcat-baseline"
```

Launching the displaCy render worker:

```bash
python -m coach_spacy.render --model en_core_web_sm --host 127.0.0.1 --port 8100
```

## Compatibility targets

- Python >= 3.13 (the SDK wheel is `abi3-py313`, so it also runs on future
  3.13+ interpreters without rebuilds).
- spaCy >= 3.8, designed so the logger keys defensively on score/scores shapes
  and tolerates 3.9/4.x changes (see `docs/SPACY_INTEGRATION.md`).
- No chromium anywhere: displaCy PNG export uses pure-Python cairosvg, and
  `displacy.serve` is never used — all rendering goes through the render
  worker.
