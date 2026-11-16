# coach_spacy

The only Python layer in AI Small Modell Coach. Zero C extensions of its own;
the ingest SDK is a Rust `abi3-py313` wheel (`coach`, built from
`crates/coach-pyo3` with maturin).

- `coach_spacy.logger` — spaCy training logger (`@loggers = "coach_spacy.logger:coach_logger"`).
- `python -m coach_spacy.render` — displaCy render worker (FastAPI/uvicorn).
