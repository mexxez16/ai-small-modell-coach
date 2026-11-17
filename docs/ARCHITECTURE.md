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
│ Run query AST         │         └─────────────────────────┘
│ SQLite (WAL)          │
└───────▲───────────────┘
        │ abi3 wheel (PyO3/maturin)
┌───────┴───────────────┐
│ crates/coach-pyo3     │  → python/coach_spacy (logger + glue)
│ coach ingest SDK      │
└───────────────────────┘
```

## Decisions

### Storage: embedded SQLite — no RocksDB

All entities (runs, metric points, image metadata, curation items, audit
history) persist to a single SQLite database at
`<COACH_DATA_DIR>/coach.db` (`rusqlite`, bundled C amalgamation). Consequences:

- No RocksDB-class native dependency — **no native wheel blocker on
  Python >= 3.13** ever, because the bundled SQLite C amalgamation compiles
  anywhere and never lags new Python releases.
- Every mutation is an incremental write (WAL journal, `synchronous=NORMAL`,
  prepared statements) — no whole-store rewrites when training metrics stream
  in.
- SQL indexes cover the dominant query patterns (metric series by run,
  curation items by dataset/status, history by item). Ad-hoc analytics over
  the same file stays possible via the standard `sqlite3` CLI or any
  SQLite-capable tooling (pandas, DuckDB over its sqlite3 extension).
- Escape hatch: if metric volumes ever outgrow SQLite (tens of millions of
  points with heavy cross-run analytics), a columnar Parquet + DuckDB layer
  slots in behind the (uncompiled) `duckdb-backend` cargo feature without
  changing the `Storage` call sites. It is deliberately not built today —
  one storage engine means one source of truth.
- Legacy compatibility: per-file JSON snapshots from the pre-SQLite scaffold
  (`sessions.json`, `metrics.json`, …) are imported automatically on first
  boot and left as `snapshot-migrated/`.

### The Python layer is tiny and shippable

Python exists only where spaCy lives:

1. `coach_spacy.logger` — the training logger (`spacy_loggers` entry point
   `coach.Logger.v1`, usable directly as `coach_spacy.logger:coach_logger`).
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
