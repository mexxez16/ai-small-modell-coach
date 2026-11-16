import { useEffect, useState } from "react";

import { api, RenderWorker } from "../api/client";

export default function Settings() {
  const [modelPath, setModelPath] = useState("en_core_web_sm");
  const [workers, setWorkers] = useState<RenderWorker[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = () =>
    api
      .listWorkers()
      .then(setWorkers)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));

  useEffect(() => {
    void refresh();
  }, []);

  const spawn = async () => {
    try {
      setError(null);
      await api.spawnWorker(modelPath);
      refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div>
      <div className="page-header">
        <h1>Settings</h1>
        <p>Render workers, workspace scope and appearance.</p>
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>Render workers</h3>
        <div className="field-row">
          <input
            className="text-input"
            style={{ minWidth: 280, fontFamily: "var(--font-mono)" }}
            value={modelPath}
            onChange={(event) => setModelPath(event.target.value)}
            placeholder="en_core_web_sm or /path/to/model"
            aria-label="spaCy model path"
          />
          <button className="button button-primary" onClick={() => void spawn()}>
            Spawn worker
          </button>
        </div>
        {error && <div className="empty-state">{error}</div>}
        <table className="table">
          <thead>
            <tr>
              <th>Model</th>
              <th>Status</th>
              <th>PID</th>
              <th>Started</th>
            </tr>
          </thead>
          <tbody>
            {workers.map((worker) => (
              <tr key={worker.id}>
                <td style={{ fontFamily: "var(--font-mono)" }}>{worker.model_path}</td>
                <td>
                  <span className="badge badge-neutral">{worker.status}</span>
                </td>
                <td>{worker.pid ?? "—"}</td>
                <td>{new Date(worker.started_at).toLocaleString()}</td>
              </tr>
            ))}
            {workers.length === 0 && (
              <tr>
                <td colSpan={4}>No workers running.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="card" style={{ marginTop: 16 }}>
        <h3 style={{ marginTop: 0 }}>Workspace scope</h3>
        <p className="stat-label">
          The embedded Vibe agent and terminal are always scoped to the
          workspace directory coach-api was started with; no host-wide access.
        </p>
      </div>
    </div>
  );
}
