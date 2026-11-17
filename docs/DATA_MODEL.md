# Data Model

Entities (Rust structs in `crates/coach-core/src/model.rs`). Timestamps are
RFC 3339 / `DateTime<Utc>`; identifiers are UUIDv4 strings.

## Sessions and runs

```
Session 1 ──n Run 1 ──n MetricPoint
                ├──n Image
                └──n Visualization (attachable from the Visualize page)
```

### Session
| field | type | notes |
|---|---|---|
| id | uuid | |
| name | string | unique in UI list |
| description | string | |
| status | active, archived | archived sessions filter out of the default view |
| tags | string[] | |
| created_at / updated_at | timestamp | |

### Run
| field | type | notes |
|---|---|---|
| id | uuid | |
| name | string | free-form, e.g. `textcat-baseline` |
| session_id | uuid, nullable | drag-and-drop assignment in the UI |
| status | running, completed, failed | logger sets completed on finalize |
| config | json, nullable | interpolated spaCy `nlp.config` |
| hparams | json | flattened key hyperparameters |
| dataset_id | uuid, nullable | dataset used |
| dataset_hash | string, nullable | sha256 of the dataset snapshot |
| tags | string[] | |

### MetricPoint
| field | type | notes |
|---|---|---|
| run_id | uuid | |
| name | string | e.g. `textcat_loss`, `score`, `cats_score` |
| context | json | discriminator, e.g. `{"type": "loss"}` |
| step / epoch | u64 / u64? | |
| value | f64 | |
| timestamp | timestamp | ingest time |

A metric series is `(run_id, name, canonical(context))`; charting groups on it.

### Image
| field | type | notes |
|---|---|---|
| id | uuid | |
| run_id / session_id | uuid, nullable | nullable for ad-hoc visual PNG exports |
| name | string | e.g. `viz-ent-step-40` |
| step / epoch | u64? | training context |
| png_base64 | string | PNG bytes (blob store on disk; base64 in the API model) |
| caption | string | |
| meta | json | format, style, model |

### Visualization
Ad-hoc or attached displaCy renders (raw SVG text, distinct from PNG Images):
id, `run_id?`, `session_id?`, `worker_model`, `text?` or `dataset_item_id?`,
`style` (ent/dep/span), `options` (json), `svg`, `created_at`. Saved from the
Visualize page via "attach to run/session".

## Datasets and curation

### Dataset
| field | type | notes |
|---|---|---|
| id | uuid | |
| name | string | |
| source_ref | string | provenance, e.g. `sources/entity_classification.json#sha256:…` |
| schema_version | u32 | DataItem schema version |
| versions | DatasetVersion[] | hash chain of the dataset states |

### DataItem
| field | type | notes |
|---|---|---|
| id | uuid | |
| dataset_id | uuid | |
| text / subject / predicate / object | string? | input fields; triples use s/p/o |
| label | string | **canonical kebab-case**, single label space across tracks |
| kind | example, negative, contrastive, pair, other(String) | |
| source | string | e.g. `entity_classification.json#E27` |
| status | raw → curated → approved / rejected | review state machine |
| split | train / dev / eval / unassigned | |
| assignee / notes | string? | |
| updated_at | timestamp | |

Status transitions are validated by the curation state machine (raw → curated
→ approved or rejected; rejected can return to curated). Every transition
appends a `DataItemHistory` row.

### DataItemHistory (audit trail)
| field | type |
|---|---|
| id | uuid |
| item_id | uuid |
| timestamp | timestamp |
| actor | string |
| field | string |
| old_value / new_value | json |

### ConversionJob
| field | type | notes |
|---|---|---|
| id | uuid | |
| dataset_id | uuid | |
| pipeline | string[] | e.g. `["source→jsonl", "jsonl→docbin"]` |
| params | json | `{dev_ratio, seed, stratify: true}` |
| status | pending, running, completed, failed | progress streams over `/ws/jobs` |
| artifacts | json[] | file refs + sha256 |
| stats | json | counts per label/split, preview diff stats |

## RenderWorker (ephemeral)

id, model_path, port, pid, status (starting/ready/unhealthy/stopped),
started_at. Managed by coach-api, **not persisted** — respawned on demand.

## Training-data pipeline (sources → chunks → annotations → export)

- `source_docs`: id, name, kind, size, sha256, content, created_at. Every
  uploaded document is stored with its hash so chunking is reproducible and
  provenance is auditable. Items produced from a source carry
  `source = "<source-name>#<chunk-index>"`.
- Practices enforced by the pipeline:
  - Chunking is deterministic over the stored content (strategies: paragraph,
    sentence, window with overlap); duplicates within a dataset are skipped.
  - AI annotation (`POST /api/annotations`) resolves items by id, calls an
    OpenAI-compatible provider, validates the returned label against the
    allowed label set, writes the item back with status raw (draft) and
    records the change in `item_history` with actor `ai:<provider>` — the
    model never sets approved.
- Export (`GET /api/datasets/{id}/export?status=…`) yields one JSON line per
  item `{text, label, type: "textcat", meta: {item_id, source, status}}` —
  the input format `spacy convert ... --converter json` accepts. Only items
  whose status matches the filter (default: all; UI defaults to
  `status=approved`) are exported.

## Storage mapping

### Review + validation + DocBin export (pipeline stages 4-5)

- `GET /api/datasets/{id}/validation` reports export-blocking schema issues
  (empty text/label, non-kebab labels, single-label datasets, conflicting
  texts) plus the per-status item counts to drive the review badges.
- `GET /api/datasets/{id}/export/docbin` forwards the curated item records to
  the render worker (`POST /convert_docbin`), which tokenizes with the loaded
  pipeline (`nlp.make_doc`) and serializes a `DocBin` per split — the label
  supervision stays aligned with the later `spacy train` tokenization. The
  binary response is the `.spacy` file; no Rust-side re-tokenization.

Rust structs persist to the single SQLite database (`coach.db`); each entity
maps to one table (`sessions`, `runs`, `metric_points`, `images`,
`visualizations`, `datasets`, `data_items`, `item_history`,
`conversion_jobs`). Column names follow the Rust field names in snake_case;
nested values (hparams, options, context) are stored as JSON strings.

## Model registry + run/dataset binding (schema v4)

- `models`: id, name, path (spaCy hub name or pipeline dir with meta.json +
  config.cfg), kind (shipped/local/finetuned), run_id (for finetuned
  outputs), notes, created_at. REST: `GET/POST /api/models`,
  `DELETE /api/models/{id}`.
- `runs` gained a `model` column plus `dataset_id` on create — a run
  explicitly binds Model + Training-Data-session. The Runs table exposes
  both as filter columns; Sessions let you create runs with those picks.
- `datasets` gained `tags`; the Training-Data-Sessions view groups datasets
  via `session:<name>` tags (`POST /api/datasets/{id}/tags`).
- All migrations are additive and verified against a v2 database upgrade.
