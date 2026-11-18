import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import {
  api,
  type DataRun,
  type DataSession,
} from "../api/client";

/**
 * Training-data sessions: groups of data-runs sharing one curation goal.
 * A session bundles several runs (one per pipeline variant or dataset);
 * each run is a configurable pipeline executed against sources.
 */
export default function DataSessions() {
  const [sessions, setSessions] = useState<DataSession[]>([]);
  const [runsBySession, setRunsBySession] = useState<Record<string, DataRun[]>>({});
  const [name, setName] = useState("");
  const [notes, setNotes] = useState("");
  const [search, setSearch] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await api.listDataSessions();
      setSessions(list);
      const maps: Record<string, DataRun[]> = {};
      await Promise.all(
        list.map(async (s) => {
          try {
            maps[s.id] = await api.listDataSessionRuns(s.id);
          } catch {
            maps[s.id] = [];
          }
        }),
      );
      setRunsBySession(maps);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const createSession = async () => {
    if (!name.trim()) return;
    setBusy(true);
    try {
      await api.createDataSession(name.trim(), notes);
      setName("");
      setNotes("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const removeSession = async (id: string) => {
    try {
      await api.deleteDataSession(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const visible = sessions.filter((s) =>
    search.trim()
      ? s.name.toLowerCase().includes(search.trim().toLowerCase()) ||
        s.notes.toLowerCase().includes(search.trim().toLowerCase())
      : true,
  );

  return (
    <div>
      <div className="page-header">
        <h1>Training Data Sessions</h1>
        <p>
          Gruppen von Kuratierungs-Pipelines: jede Session bündelt mehrere Data-Runs
          (z.B. Varianten derselben Quelle) bis zu fertig kuratierten Trainingsdaten.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3>Neue Session anlegen</h3>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
          <input
            className="input"
            placeholder="Session-Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void createSession()}
            style={{ width: 260 }}
          />
          <input
            className="input"
            placeholder="Notizen (optional)"
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            style={{ width: 320 }}
          />
          <button
            className="button button-primary"
            disabled={busy || !name.trim()}
            onClick={() => void createSession()}
          >
            Session anlegen
          </button>
        </div>
      </div>

      <div className="field-row" style={{ marginBottom: 8 }}>
        <input
          className="input"
          placeholder="Sessions filtern…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 240 }}
        />
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Notizen</th>
            <th>Runs</th>
            <th>Aktualisiert</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {visible.length === 0 && (
            <tr>
              <td colSpan={5}>
                Noch keine Data-Sessions — oben anlegen, dann Runs hinzufügen.
              </td>
            </tr>
          )}
          {visible.map((s) => {
            const runs = runsBySession[s.id] ?? [];
            return (
              <tr key={s.id}>
                <td>{s.name}</td>
                <td style={{ maxWidth: 320, overflow: "hidden", textOverflow: "ellipsis" }}>
                  {s.notes || "—"}
                </td>
                <td>{runs.length}</td>
                <td>{new Date(s.updated_at).toLocaleString()}</td>
                <td>
                  <div className="field-row" style={{ gap: 6, marginBottom: 0 }}>
                    <Link className="button" to={`/training-data/sessions/${s.id}`}>
                      Öffnen
                    </Link>
                    <button
                      className="button button-ghost"
                      onClick={() => void removeSession(s.id)}
                      title="Session löschen (Runs bleiben erhalten)"
                    >
                      Löschen
                    </button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
