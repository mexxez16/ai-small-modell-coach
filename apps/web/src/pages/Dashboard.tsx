import { useEffect, useState } from "react";

import { api, Health, Run, Session } from "../api/client";

export default function Dashboard() {
  const [health, setHealth] = useState<Health | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [h, s, r] = await Promise.all([api.health(), api.listSessions(), api.listRuns()]);
        if (cancelled) return;
        setHealth(h);
        setSessions(s);
        setRuns(r);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div>
      <div className="page-header">
        <h1>Dashboard</h1>
        <p>Coach status, recent sessions and runs at a glance.</p>
      </div>

      {error && (
        <div className="empty-state">
          <h3>API unreachable</h3>
          <p>{error} Start the backend with `make serve`.</p>
        </div>
      )}

      <div className="card-grid">
        <div className="card">
          <p className="stat-value">{health ? `${health.status} v${health.version}` : "…"}</p>
          <p className="stat-label">API server</p>
        </div>
        <div className="card">
          <p className="stat-value">{sessions.length}</p>
          <p className="stat-label">Sessions</p>
        </div>
        <div className="card">
          <p className="stat-value">{runs.length}</p>
          <p className="stat-label">Runs</p>
        </div>
        <div className="card">
          <p className="stat-value">0</p>
          <p className="stat-label">Curated datasets</p>
        </div>
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>Recent runs</h3>
        <table className="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Status</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {runs.slice(0, 8).map((run) => (
              <tr key={run.id}>
                <td>{run.name}</td>
                <td>
                  <span className={`badge badge-${run.status === "completed" ? "success" : "neutral"}`}>
                    {run.status}
                  </span>
                </td>
                <td>{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {runs.length === 0 && (
              <tr>
                <td colSpan={3}>No runs yet — train a spaCy pipeline with the coach logger.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
