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
| source_ref | string | provenance, e.g. `ssot/BFO_Entity_Classification.json#sha256:…` |
| schema_version | u32 | DataItem schema version |
| versions | DatasetVersion[] | hash chain of the SSOT-derived dataset states |

### DataItem
| field | type | notes |
|---|---|---|
| id | uuid | |
| dataset_id | uuid | |
| text / subject / predicate / object | string? | input fields; triples use s/p/o |
| label | string | **canonical kebab-case**, single label space across tracks |
| kind | example, negative, contrastive, pair, other(String) | |
| source | string | e.g. `BFO_Entity_Classification.json#E27` |
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
| pipeline | string[] | e.g. `["ssot→jsonl", "jsonl→docbin"]` |
| params | json | `{dev_ratio, seed, stratify: true}` |
| status | pending, running, completed, failed | progress streams over `/ws/jobs` |
| artifacts | json[] | file refs + sha256 |
| stats | json | counts per label/split, preview diff stats |

## RenderWorker (ephemeral)

id, model_path, port, pid, status (starting/ready/unhealthy/stopped),
started_at. Managed by coach-api, **not persisted** — respawned on demand.

## Storage mapping

Rust structs serialize to Parquet (metrics heavy table) and JSON-sidecar
metadata per run, as documented in `crates/coach-core/src/storage.rs`
(`parquet_layout` module).
