import { useCallback, useEffect, useState } from "react";

import { api, Run, Session } from "../api/client";

export default function Sessions() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const [s, r] = await Promise.all([api.listSessions(), api.listRuns()]);
      setSessions(s);
      setRuns(r);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const createSession = async () => {
    if (!name.trim()) return;
    try {
      await api.createSession(name.trim());
      setName("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const runsOf = (sessionId: string) => runs.filter((r) => r.session_id === sessionId);

  return (
    <div>
      <div className="page-header">
        <h1>Sessions</h1>
        <p>Sessions group and compare training runs.</p>
      </div>

      <div className="field-row">
        <input
          className="text-input"
          placeholder="New session name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void createSession()}
        />
        <button className="button button-primary" onClick={() => void createSession()}>
          Create session
        </button>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="card">
        <table className="table">
          <thead>
            <tr>
              <th>Session</th>
              <th>Status</th>
              <th>Runs</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {sessions.map((session) => (
              <tr key={session.id} onClick={() => setSelected(session.id)} style={{ cursor: "pointer" }}>
                <td>{session.name}</td>
                <td>
                  <span className="badge badge-neutral">{session.status}</span>
                </td>
                <td>{runsOf(session.id).length}</td>
                <td>{new Date(session.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {sessions.length === 0 && !error && (
              <tr>
                <td colSpan={4}>No sessions yet. Create one above.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {selected && (
        <div className="card">
          <h3 style={{ marginTop: 0 }}>Runs in “{sessions.find((s) => s.id === selected)?.name}”</h3>
          <ul>
            {runsOf(selected).map((run) => (
              <li key={run.id}>
                {run.name} <span className="badge badge-neutral">{run.status}</span>
              </li>
            ))}
            {runsOf(selected).length === 0 && <li>No runs assigned yet.</li>}
          </ul>
        </div>
      )}
    </div>
  );
}
