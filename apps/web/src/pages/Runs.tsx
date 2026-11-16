import { useCallback, useEffect, useState } from "react";

import { api, Run } from "../api/client";

type QueryResult = {
  parsed: string;
  note: string;
  matched_runs: Run[];
};

export default function Runs() {
  const [runs, setRuns] = useState<Run[]>([]);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setRuns(await api.listRuns());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runQuery = async () => {
    try {
      setError(null);
      const res = await api.queryRuns(query);
      setResult(res);
    } catch (err) {
      setResult(null);
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div>
      <div className="page-header">
        <h1>Runs</h1>
        <p>
          All training runs. Query box speaks the AimQL-like language, e.g.{" "}
          <code>run.name == "baseline" and metrics.cats_score &gt; 0.9</code>.
        </p>
      </div>

      <div className="field-row">
        <input
          className="text-input"
          style={{ minWidth: 380, fontFamily: "var(--font-mono)" }}
          placeholder='run.name == "x" and metrics.cats_score > 0.9'
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void runQuery()}
        />
        <button className="button button-primary" onClick={() => void runQuery()}>
          Query
        </button>
      </div>

      {result && (
        <div className="card">
          <p className="stat-label">Parsed query</p>
          <p style={{ fontFamily: "var(--font-mono)" }}>{result.parsed || "(match all)"}</p>
          <p className="stat-label">
            Matched runs: {result.matched_runs.length}
          </p>
        </div>
      )}
      {error && <div className="empty-state">{error}</div>}

      <div className="card">
        <table className="table">
          <thead>
            <tr>
              <th>Run</th>
              <th>Status</th>
              <th>Tags</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => (
              <tr key={run.id}>
                <td>{run.name}</td>
                <td>
                  <span className={`badge badge-${run.status === "completed" ? "success" : run.status === "failed" ? "danger" : "neutral"}`}>
                    {run.status}
                  </span>
                </td>
                <td>{run.tags.join(", ") || "—"}</td>
                <td>{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {runs.length === 0 && !error && (
              <tr>
                <td colSpan={4}>No runs recorded yet.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
