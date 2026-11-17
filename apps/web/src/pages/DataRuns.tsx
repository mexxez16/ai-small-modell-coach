import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import HelpBubble from "../components/HelpBubble";
import {
  api,
  type DataRun,
  type DataRunStage,
  type DataSession,
  type SourceDoc,
} from "../api/client";

/**
 * Training-data runs: each run is a freely configurable curation pipeline
 * (ssot_import → chunk → ai_annotate → export). This page lists all runs
 * with filters and hosts the per-run stage editor + execution results.
 */

const STAGE_KINDS = ["ssot_import", "chunk", "ai_annotate", "export"] as const;
const STATUS_OPTIONS = ["draft", "running", "completed", "failed"];

type SortKey = "name" | "status" | "session" | "updated_at";

function newDefaultConfig(kind: (typeof STAGE_KINDS)[number]): Record<string, unknown> {
  switch (kind) {
    case "ssot_import":
      return { source_id: "", label_overrides: {} };
    case "chunk":
      return { source_id: "", strategy: "paragraph", max_chars: 600, overlap: 0, label: "imported-chunk" };
    case "ai_annotate":
      return { provider_id: "", instruction: "", labels: [], kinds: ["example"], limit: 0 };
    case "export":
      return { heads: [{ name: "type", kinds: ["example"] }] };
  }
}

export default function DataRuns() {
  const { runId } = useParams<{ runId?: string }>();
  const navigate = useNavigate();
  const [, setSearchParams] = useSearchParams();
  const [runs, setRuns] = useState<DataRun[]>([]);
  const [sessions, setSessions] = useState<DataSession[]>([]);
  const [sources, setSources] = useState<SourceDoc[]>([]);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [sessionFilter, setSessionFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("updated_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  const [newRunName, setNewRunName] = useState("");
  const [newRunSession, setNewRunSession] = useState("");

  // Stage-editor state for the selected run.
  const [stages, setStages] = useState<DataRunStage[]>([]);
  const [providerId, setProviderId] = useState("");
  const [providers, setProviders] = useState<Array<{ id: string; name: string }>>([]);

  const detail = useMemo(() => runs.find((r) => r.id === runId) ?? null, [runs, runId]);

  const refresh = useCallback(async () => {
    try {
      const [r, s] = await Promise.all([api.listDataRuns(), api.listDataSessions()]);
      setRuns(r);
      setSessions(s);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
    api.listSources().then(setSources).catch(() => {});
    fetch("/api/settings/providers")
      .then((r) => r.json() as Promise<{ providers?: Array<{ id: string; name: string }> }>)
      .then((d) => setProviders(d.providers ?? []))
      .catch(() => {});
  }, [refresh]);

  // Load stages of the selected run into the editor.
  useEffect(() => {
    if (detail && Array.isArray(detail.stages)) {
      setStages(detail.stages.map((s) => ({ kind: s.kind, config: { ...s.config } })));
    } else {
      setStages([]);
    }
  }, [detail]);

  const createRun = async () => {
    if (!newRunName.trim()) return;
    setBusy(true);
    try {
      const run = await api.createDataRun(newRunName.trim(), newRunSession || undefined, []);
      setNewRunName("");
      await refresh();
      navigate(`/training-data/runs/${run.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const executeRun = async (id: string) => {
    setBusy(true);
    setResult(null);
    try {
      const run = await api.executeDataRun(id);
      const stageResults = run.results ?? {};
      const summary = Object.entries(stageResults)
        .filter(([k]) => k !== "errors")
        .map(([k, v]) => {
          const obj = v as Record<string, unknown>;
          return `${k}: added=${obj.added ?? "—"} heads=${Array.isArray(obj.heads) ? obj.heads.length : "—"}`;
        })
        .join(" · ");
      setResult(summary || `Status: ${run.status}`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const deleteRun = async (id: string) => {
    try {
      await api.deleteDataRun(id);
      if (runId === id) setSearchParams({});
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const saveStages = async () => {
    if (!detail) return;
    setBusy(true);
    try {
      await api.updateDataRun(detail.id, { stages });
      await refresh();
      setResult("Stages gespeichert.");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const updateStage = (idx: number, patch: Partial<DataRunStage>) => {
    setStages((prev) => prev.map((s, i) => (i === idx ? { ...s, ...patch } : s)));
  };

  const updateConfig = (idx: number, patch: Record<string, unknown>) => {
    setStages((prev) =>
      prev.map((s, i) => (i === idx ? { ...s, config: { ...s.config, ...patch } } : s)),
    );
  };

  const moveStage = (idx: number, dir: -1 | 1) => {
    setStages((prev) => {
      const next = [...prev];
      const target = idx + dir;
      if (target < 0 || target >= next.length) return prev;
      [next[idx], next[target]] = [next[target], next[idx]];
      return next;
    });
  };

  const visible = runs
    .filter((r) => (statusFilter ? r.status === statusFilter : true))
    .filter((r) => (sessionFilter ? r.session_id === sessionFilter : true))
    .filter((r) =>
      search.trim() ? r.name.toLowerCase().includes(search.trim().toLowerCase()) : true,
    )
    .sort((a, b) => {
      const dir = sortAsc ? 1 : -1;
      const va =
        sortKey === "session"
          ? sessions.find((s) => s.id === a.session_id)?.name ?? ""
          : sortKey === "updated_at"
            ? a.updated_at
            : a[sortKey];
      const vb =
        sortKey === "session"
          ? sessions.find((s) => s.id === b.session_id)?.name ?? ""
          : sortKey === "updated_at"
            ? b.updated_at
            : b[sortKey];
      return va < vb ? -dir : va > vb ? dir : 0;
    });

  const sortHeader = (key: SortKey, label: string) => (
    <th
      style={{ cursor: "pointer" }}
      onClick={() => {
        if (sortKey === key) setSortAsc((v) => !v);
        else {
          setSortKey(key);
          setSortAsc(true);
        }
      }}
    >
      {label} {sortKey === key ? (sortAsc ? "↑" : "↓") : ""}
    </th>
  );

  return (
    <div>
      <div className="page-header">
        <h1>
          Training Data Runs <HelpBubble helpId="data-runs.pipeline" />
        </h1>
        <p>
          Jeder Run ist eine frei konfigurierbare Kuratierungs-Pipeline von der Quelle
          (z.B. SSOT-Kopien) bis zu exportfähigen Trainingsdaten.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {result && <div className="stat-label">{result}</div>}

      {!runId && (
        <div className="card" style={{ marginBottom: 16 }}>
          <h3>Neuen Run anlegen</h3>
          <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
            <input
              className="input"
              placeholder="Run-Name"
              value={newRunName}
              onChange={(e) => setNewRunName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void createRun()}
              style={{ width: 260 }}
            />
            <select
              className="select"
              value={newRunSession}
              onChange={(e) => setNewRunSession(e.target.value)}
              aria-label="Session"
            >
              <option value="">— Session zuordnen —</option>
              {sessions.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
            <button
              className="button button-primary"
              disabled={busy || !newRunName.trim()}
              onClick={() => void createRun()}
            >
              Run anlegen
            </button>
          </div>
        </div>
      )}

      {!runId && (
        <div className="field-row" style={{ marginBottom: 8, flexWrap: "wrap", gap: 8 }}>
          <input
            className="input"
            placeholder="Runs filtern…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ width: 200 }}
          />
          <select
            className="select"
            value={statusFilter}
            onChange={(e) => setStatusFilter(e.target.value)}
            aria-label="Status-Filter"
          >
            <option value="">— Status —</option>
            {STATUS_OPTIONS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
          <select
            className="select"
            value={sessionFilter}
            onChange={(e) => setSessionFilter(e.target.value)}
            aria-label="Session-Filter"
          >
            <option value="">— alle Sessions —</option>
            {sessions.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {!runId && (
        <table className="table">
          <thead>
            <tr>
              {sortHeader("name", "Run")}
              {sortHeader("session", "Session")}
              {sortHeader("status", "Status")}
              <th>Stages</th>
              <th>Dataset</th>
              {sortHeader("updated_at", "Aktualisiert")}
              <th />
            </tr>
          </thead>
          <tbody>
            {visible.length === 0 && (
              <tr>
                <td colSpan={7}>Keine Runs gefunden.</td>
              </tr>
            )}
            {visible.map((run) => (
              <tr key={run.id}>
                <td>{run.name}</td>
                <td>{sessions.find((s) => s.id === run.session_id)?.name ?? "—"}</td>
                <td>
                  <span className="badge badge-neutral">{run.status}</span>
                </td>
                <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                  {Array.isArray(run.stages) ? run.stages.map((s) => s.kind).join(" → ") : "—"}
                </td>
                <td>{run.dataset_id ? `${run.dataset_id.slice(0, 8)}…` : "—"}</td>
                <td>{new Date(run.updated_at).toLocaleString()}</td>
                <td className="field-row" style={{ gap: 6 }}>
                  <Link className="button" to={`/training-data/runs/${run.id}`}>
                    Pipeline
                  </Link>
                  <button
                    className="button button-primary"
                    disabled={busy}
                    onClick={() => void executeRun(run.id)}
                  >
                    Ausführen
                  </button>
                  <button
                    className="button button-ghost"
                    onClick={() => void deleteRun(run.id)}
                  >
                    Löschen
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {detail && (
        <div className="card">
          <div className="field-row" style={{ justifyContent: "space-between" }}>
            <h3 style={{ margin: 0 }}>
              Pipeline: {detail.name}{" "}
              <span className="badge badge-neutral">{detail.status}</span>
            </h3>
            <div className="field-row" style={{ gap: 6 }}>
              <button
                className="button"
                onClick={() => {
                  setSearchParams({});
                }}
              >
                ← Zu allen Runs
              </button>
              <button
                className="button"
                disabled={busy || stages.length === 0}
                onClick={() => void saveStages()}
              >
                Nur speichern
              </button>
              <button
                className="button button-primary"
                disabled={busy || stages.length === 0}
                onClick={async () => {
                  await saveStagesAndExecute();
                }}
              >
                Speichern + Ausführen
              </button>
            </div>
          </div>

          <p className="stat-label">
            Dataset: {detail.dataset_id ? (
              <a href={`/api/datasets/${detail.dataset_id}/export`} download>
                {detail.dataset_id.slice(0, 8)}… (JSONL)
              </a>
            ) : (
              "wird bei der ersten Ausführung erzeugt"
            )}
          </p>

          {stages.length === 0 && (
            <div className="empty-state">Noch keine Stages — unten hinzufügen.</div>
          )}

          {stages.map((stage, idx) => (
            <div
              key={idx}
              className="card"
              style={{ margin: "10px 0", background: "var(--bg-elev-1, transparent)" }}
            >
              <div className="field-row" style={{ flexWrap: "wrap", gap: 8, alignItems: "center" }}>
                <b>#{idx + 1}</b>
                <select
                  className="select"
                  value={stage.kind}
                  onChange={(e) =>
                    updateStage(idx, {
                      kind: e.target.value as DataRunStage["kind"],
                      config: newDefaultConfig(e.target.value as (typeof STAGE_KINDS)[number]),
                    })
                  }
                  aria-label="Stage-Typ"
                >
                  {STAGE_KINDS.map((k) => (
                    <option key={k} value={k}>
                      {k}
                    </option>
                  ))}
                </select>
                <button className="button" onClick={() => moveStage(idx, -1)} title="nach oben">
                  ↑
                </button>
                <button className="button" onClick={() => moveStage(idx, 1)} title="nach unten">
                  ↓
                </button>
                <button
                  className="button button-ghost"
                  onClick={() => setStages((prev) => prev.filter((_, i) => i !== idx))}
                >
                  Entfernen
                </button>
              </div>

              {stage.kind === "ssot_import" && (
                <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 8 }}>
                  <select
                    className="select"
                    value={String(stage.config.source_id ?? "")}
                    onChange={(e) => updateConfig(idx, { source_id: e.target.value })}
                    aria-label="Quelle"
                    title="Strukturierter JSON-Import: entities/relations → Items"
                  >
                    <option value="">— Quelle wählen —</option>
                    {sources.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                  <span className="stat-label">
                    Entities: examples → example · negative_examples → negative · contrastive_examples → contrastive; Relations-Beispiele → Tripel (Paare)
                  </span>
                </div>
              )}

              {stage.kind === "chunk" && (
                <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 8 }}>
                  <select
                    className="select"
                    value={String(stage.config.source_id ?? "")}
                    onChange={(e) => updateConfig(idx, { source_id: e.target.value })}
                    aria-label="Quelle"
                  >
                    <option value="">— Quelle wählen —</option>
                    {sources.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                  <select
                    className="select"
                    value={String(stage.config.strategy ?? "paragraph")}
                    onChange={(e) => updateConfig(idx, { strategy: e.target.value })}
                    aria-label="Strategie"
                  >
                    <option value="paragraph">Absatz</option>
                    <option value="sentence">Satz</option>
                    <option value="window">Fenster</option>
                  </select>
                  <input
                    className="input"
                    type="number"
                    min={40}
                    value={Number(stage.config.max_chars ?? 600)}
                    onChange={(e) => updateConfig(idx, { max_chars: Number(e.target.value) })}
                    title="max. Zeichen pro Chunk"
                    style={{ width: 80 }}
                  />
                  <input
                    className="input"
                    type="number"
                    min={0}
                    value={Number(stage.config.overlap ?? 0)}
                    onChange={(e) => updateConfig(idx, { overlap: Number(e.target.value) })}
                    title="Überlappung (nur Fenster)"
                    style={{ width: 70 }}
                  />
                  <input
                    className="input"
                    placeholder="Label (kebab-case)"
                    value={String(stage.config.label ?? "")}
                    onChange={(e) => updateConfig(idx, { label: e.target.value })}
                    style={{ width: 160 }}
                  />
                </div>
              )}

              {stage.kind === "ai_annotate" && (
                <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 8 }}>
                  <select
                    className="select"
                    value={String(stage.config.provider_id ?? providerId)}
                    onChange={(e) => {
                      updateConfig(idx, { provider_id: e.target.value });
                      setProviderId(e.target.value);
                    }}
                    aria-label="Provider"
                  >
                    <option value="">— Provider wählen —</option>
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  <input
                    className="input"
                    placeholder="Instruktion für das Modell"
                    value={String(stage.config.instruction ?? "")}
                    onChange={(e) => updateConfig(idx, { instruction: e.target.value })}
                    style={{ width: 300 }}
                  />
                  <input
                    className="input"
                    placeholder="Labels (Komma-getrennt, leer = aus Datensatz)"
                    value={Array.isArray(stage.config.labels) ? stage.config.labels.join(", ") : ""}
                    onChange={(e) =>
                      updateConfig(idx, {
                        labels: e.target.value
                          .split(",")
                          .map((s) => s.trim())
                          .filter(Boolean),
                      })
                    }
                    style={{ width: 260 }}
                  />
                  <input
                    className="input"
                    type="number"
                    min={0}
                    value={Number(stage.config.limit ?? 0)}
                    onChange={(e) => updateConfig(idx, { limit: Number(e.target.value) })}
                    title="Limit (0 = alle passenden Items)"
                    style={{ width: 70 }}
                  />
                </div>
              )}

              {stage.kind === "export" && (
                <div className="stat-label" style={{ marginTop: 6 }}>
                  Standard-Heads: type (examples) · relation (Pairs mit &lt;SEP&gt;) ·
                  relevance (examples + negatives). Pro Head wird ein JSONL-Artefakt erzeugt.
                </div>
              )}
            </div>
          ))}

          <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 8 }}>
            {STAGE_KINDS.map((k) => (
              <button
                key={k}
                className="button"
                onClick={() =>
                  setStages((prev) => [...prev, { kind: k as DataRunStage["kind"], config: newDefaultConfig(k) }])
                }
              >
                + {k}
              </button>
            ))}
          </div>

          {detail.results && Object.keys(detail.results).length > 0 && (
            <div style={{ marginTop: 12 }}>
              <h3>Ergebnisse der letzten Ausführung</h3>
              <table className="table">
                <thead>
                  <tr>
                    <th>Stage</th>
                    <th>Kennzahlen</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(detail.results).map(([key, value]) => (
                    <tr key={key}>
                      <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>{key}</td>
                      <td style={{ fontFamily: "var(--font-mono)", fontSize: 11, maxWidth: 520, overflow: "hidden", textOverflow: "ellipsis" }}>
                        {JSON.stringify(value)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {detail.dataset_id && (
                <Link className="button" to={`/training-data?dataset=${detail.dataset_id}`}>
                  Items kuratieren (Workbench öffnen)
                </Link>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );

  async function saveStagesAndExecute() {
    if (!detail) return;
    setBusy(true);
    try {
      await api.updateDataRun(detail.id, { stages });
      await executeRun(detail.id);
    } finally {
      setBusy(false);
    }
  }
}
