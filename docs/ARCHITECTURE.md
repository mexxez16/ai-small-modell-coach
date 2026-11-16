# Architecture

AI Small Modell Coach is a monorepo: a Rust backend, an own React frontend,
and a deliberately tiny Python layer for everything spaCy-specific.

```
┌───────────────────────────────── apps/web (React 18 + TS + Vite) ─────────┐
│  Dashboard  Sessions  Runs  Training Data  Visualize  Images  Settings    │
│  Right dock: Chat (collapsible) · Bottom dock: Terminal (collapsible)     │
└─────────────────────────────▲──────────────────────────────────────────────┘
                REST + WS     │  (typed client, generated from OpenAPI)
┌─────────────────────────────┴──────────────────────────────────────────────┐
│ crates/coach-api (axum): REST, /ws/* fan-out, PTY mgmt (/ws/terminal),     │
│ chat providers (OpenAI-compatible streaming + vibe PTY, see providers.rs), │
│ render-worker process mgmt, static file serving (SPA fallback)             │
└───────▲─────────────────────────────────────┬──────────────────────────────┘
        │ CRDT-free shared traits             │ process mgmt
┌───────┴───────────────┐         ┌───────────▼─────────────┐
│ crates/coach-core     │         │ python render worker    │
│ domain model          │         │ (coach_spacy.render,    │
│ storage traits        │         │  FastAPI/uvicorn, spaCy)│
│ AimQL-ish query AST   │         └─────────────────────────┘
│ Parquet + DuckDB      │
└───────▲───────────────┘
        │ abi3 wheel (PyO3/maturin)
┌───────┴───────────────┐
│ crates/coach-pyo3     │  → python/coach_spacy (logger + glue)
│ coach ingest SDK      │
└───────────────────────┘
```

## Decisions

### Storage: Parquet files + embedded DuckDB — no RocksDB

All entities (runs, metric points, images metadata, curation items, audit
history) are written as Apache Parquet files on local disk (see the layout
documented in `crates/coach-core/src/storage.rs`) and queried with an
embedded DuckDB connection. Consequences:

- No `aimrocks` and therefore **no native wheel blocker on Python >= 3.13**.
  (Upstream `aim` depends on `aimrocks`, which historically lagged new
  Python releases; this project avoids that class of problem entirely.)
- Columnar metric layout suits the dominant query pattern (series by run).
- An in-memory reference store implements the same traits and is used in
  tests and as the scaffold's default; the DuckDB backend slots in behind
  the `duckdb-backend` cargo feature without changing call sites.
- Durability: the running server snapshots the store to
  `<COACH_DATA_DIR>/*.json` (atomic temp-file + rename) and reloads it on
  boot, so restarts do not lose sessions/runs/metrics/images. The snapshot
  layer is intentionally thin; when the Parquet/DuckDB backend lands it
  replaces the file snapshot entirely.

### The Python layer is tiny and shippable

Python exists only where spaCy lives:

1. `coach_spacy.logger` — the training logger (`spacy_loggers` entry point
   `coach.AimLogger.v1`, usable directly as `coach_spacy.logger:coach_logger`).
2. `coach_spacy.render` — the displaCy render worker
   (`python -m coach_spacy.render --model …`).

Both are pure Python (spaCy itself ships cp313 wheels). The ingest SDK is the
Rust `coach-pyo3` crate built with maturin as an `abi3-py313` wheel, so a
single wheel covers 3.13 and every later interpreter.

### coach-api is the process supervisor

The Rust server owns lifecycle concerns: HTTP/WebSocket serving, PTY
allocation for the terminal, spawning and supervising the render worker, and
bridging the Mistral Vibe CLI over a PTY. None of the Python code is imported
by Rust; the contract is HTTP.

### Security posture

- Single-user bearer token (`COACH_TOKEN` env), localhost binding by default.
- displaCy markup embeds user text: options are whitelisted in the render
  worker, response sizes are capped in coach-api, and the frontend sanitizes
  with DOMPurify before inlining (see `docs/SPACY_INTEGRATION.md`).
- No telemetry, no external CDNs at runtime, self-hosted assets only.

## Feature order (post-scaffold)

ingest + storage → runs/metrics UI → sessions → training-data curation →
images → Visualize page + render worker → terminal → Vibe chat → conversion
pipeline UI.
