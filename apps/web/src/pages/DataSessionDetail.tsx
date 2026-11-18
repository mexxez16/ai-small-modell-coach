import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, type DataSession, type DataRun } from "../api/client";
import HelpBubble from "../components/HelpBubble";

const STAGE_HINTS: Record<string, string> = {
  structured_import: "Strukturierter JSON-Import (Entities/Relations → Items)",
  chunk: "Text-Chunking einer Quelle",
  ai_annotate: "KI-Annotation (OpenAI-kompatibler Provider)",
  export: "Per-Head-JSONL-Artefakte",
};

/** Detail page of one training-data session: its runs, plus creating new
 * pipeline runs bound to the session. */
export default function DataSessionDetail() {
  const { id } = useParams<{ id: string }>();
  const [session, setSession] = useState<DataSession | null>(null);
  const [runs, setRuns] = useState<DataRun[]>([]);
  const [runName, setRunName] = useState("");
  const [assignId, setAssignId] = useState("");
  const [orphans, setOrphans] = useState<DataRun[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    if (!id) return;
    try {
      const [s, r] = await Promise.all([
        api.listDataSessions().then((all) => all.find((x) => x.id === id) ?? null),
        api.listDataSessionRuns(id),
      ]);
      setSession(s);
      setRuns(r);
      setError(null);
      // Alle weiteren Runs (jede Session) können hier zugeordnet werden.
      const all = await api.listDataRuns();
      setOrphans(all.filter((x) => x.id && x.session_id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [id]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const createRun = async (preset: string) => {
    if (!id || !runName.trim()) return;
    setBusy(true);
    try {
      const stages = preset === "import" ? [{ kind: "structured_import" as const, config: {} }, { kind: "export" as const, config: {} }] : [];
      await api.createDataRun(runName.trim(), id, stages);
      setRunName("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const executeRun = async (runId: string) => {
    setBusy(true);
    try {
      await api.executeDataRun(runId);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  if (!session) {
    return <div className="empty-state">{error ?? "Lädt…"}</div>;
  }

  return (
    <div>
      <div className="page-header">
        <h1>
          Session: {session.name} <HelpBubble helpId="data-runs.pipeline" />
        </h1>
        <p>{session.notes || "Kuratierungs-Session"}</p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3>Run in dieser Session anlegen</h3>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
          <input
            className="input"
            placeholder="Run-Name, z.B. kurierung-v1"
            value={runName}
            onChange={(e) => setRunName(e.target.value)}
            style={{ width: 280 }}
          />
          <button
            className="button button-primary"
            disabled={busy || !runName.trim()}
            onClick={() => void createRun("import")}
            title="Stages: strukturierter Import + Export — im Run-Editor frei konfigurierbar"
          >
            Mit Import-Pipeline anlegen
          </button>
          <button
            className="button"
            disabled={busy || !runName.trim()}
            onClick={() => void createRun("empty")}
          >
            Leeren Run anlegen
          </button>
        </div>
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <h3>Bestehenden Run zuordnen</h3>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
          <select
            className="select"
            value={assignId}
            onChange={(e) => setAssignId(e.target.value)}
            aria-label="Bestehenden Run auswählen"
            style={{ width: 320 }}
          >
            <option value="">— Run auswählen —</option>
            {orphans.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
          </select>
          <button
            className="button button-primary"
            disabled={!assignId || busy}
            onClick={async () => {
              if (!assignId) return;
              setBusy(true);
              try {
                await api.updateDataRun(assignId, { session_id: id });
                setAssignId("");
                await refresh();
              } finally {
                setBusy(false);
              }
            }}
          >
            Zuordnen
          </button>
        </div>
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>Run</th>
            <th>Status</th>
            <th>Stages</th>
            <th>Dataset</th>
            <th>Aktualisiert</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {runs.length === 0 && (
            <tr>
              <td colSpan={6}>Noch keine Runs in dieser Session.</td>
            </tr>
          )}
          {runs.map((run) => (
            <tr key={run.id}>
              <td>{run.name}</td>
              <td>
                <span className="badge badge-neutral">{run.status}</span>
              </td>
              <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                {Array.isArray(run.stages)
                  ? run.stages.map((s) => s.kind).join(" → ")
                  : "—"}
              </td>
              <td>
                {run.dataset_id ? (
                  <Link to={`/training-data/runs/${run.id}`}>
                    {run.dataset_id.slice(0, 8)}…
                  </Link>
                ) : (
                  "—"
                )}
              </td>
              <td>{new Date(run.updated_at).toLocaleString()}</td>
              <td>
                <div className="field-row" style={{ gap: 6, marginBottom: 0 }}>
                  <Link className="button" to={`/training-data/runs/${run.id}`}>
                    Pipeline öffnen
                  </Link>
                  <button
                    className="button button-primary"
                    disabled={busy}
                    onClick={() => void executeRun(run.id)}
                  >
                    Ausführen
                  </button>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <p className="stat-label" style={{ marginTop: 8 }}>
        Stage-Arten: {Object.entries(STAGE_HINTS).map(([k, v]) => `${k} — ${v}`).join(" · ")}
      </p>
    </div>
  );
}
