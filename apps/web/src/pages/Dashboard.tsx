import { useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api, Run } from "../api/client";

type DashboardData = {
  counts: {
    sessions: number;
    runs_total: number;
    runs_running: number;
    runs_completed: number;
    runs_failed: number;
    datasets: number;
    data_items: number;
    images: number;
    visualizations: number;
  };
  recent_runs: Run[];
  metric_names: string[];
  params: Array<{ id: string; name: string; status: string; hparams: unknown }>;
};

/** Flatten nested hparams one level into dotted keys (run.lr -> lr). */
function flattenHparams(value: unknown, prefix = ""): Record<string, string> {
  const out: Record<string, string> = {};
  if (value === null || value === undefined || typeof value !== "object") {
    if (value !== undefined) out[prefix || "value"] = String(value);
    return out;
  }
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      Object.assign(out, flattenHparams(v, key));
    } else {
      out[key] = Array.isArray(v) ? v.join(", ") : String(v);
    }
  }
  return out;
}

export default function Dashboard() {
  const [data, setData] = useState<DashboardData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<
    Array<{ run: Run; name: string; points: Array<{ x: number; y: number }> }>
  >([]);

  useEffect(() => {
    let cancelled = false;
    api
      .dashboard()
      .then((d) => {
        if (cancelled) return;
        setData(d);
        // Overlay the most recent runs' most informative series — the metric
        // that the run produces least/most recently is left out; the grid
        // shows up to the first two series of each of the six recent runs.
        const jobs = d.recent_runs.slice(0, 6).map(async (run) => {
          try {
            const res = await fetch(`/api/runs/${run.id}/metrics`);
            if (!res.ok) return [];
            const pts = (await res.json()) as Array<{
              name: string;
              step: number;
              value: number;
            }>;
            const byName = new Map<string, Array<[number, number]>>();
            for (const m of pts) {
              const arr = byName.get(m.name) ?? [];
              arr.push([m.step, m.value]);
              byName.set(m.name, arr);
            }
            return [...byName.entries()]
              .slice(0, 2)
              .map(([name, arr]) => ({
                run,
                name,
                points: arr.map(([x, y]) => ({ x, y })),
              }));
          } catch {
            return [];
          }
        });
        Promise.all(jobs).then((groups) => {
          if (!cancelled) setMetrics(groups.flat());
        });
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <div>
        <div className="page-header">
          <h1>Dashboard</h1>
        </div>
        <div className="empty-state">
          <h3>API unreachable</h3>
          <p>{error} Start the backend with `make serve`.</p>
        </div>
      </div>
    );
  }

  if (!data) {
    return (
      <div>
        <div className="page-header">
          <h1>Dashboard</h1>
        </div>
        <div className="empty-state">
          <h3>Loading…</h3>
        </div>
      </div>
    );
  }

  const c = data.counts;
  const hparamKeys = [
    ...new Set(
      data.params.flatMap((p) => Object.keys(flattenHparams(p.hparams, ""))),
    ),
  ].slice(0, 8);

  return (
    <div>
      <div className="page-header">
        <h1>Dashboard</h1>
        <p>Coach status, recent sessions, runs and the metric index.</p>
      </div>

      <div className="card-grid">
        <div className="card">
          <p className="stat-value">{c.runs_total}</p>
          <p className="stat-label">Runs ({c.runs_running} running · {c.runs_completed} done · {c.runs_failed} failed)</p>
        </div>
        <div className="card">
          <p className="stat-value">{c.sessions}</p>
          <p className="stat-label">Sessions</p>
        </div>
        <div className="card">
          <p className="stat-value">{c.datasets}</p>
          <p className="stat-label">Datasets ({c.data_items} items)</p>
        </div>
        <div className="card">
          <p className="stat-value">{c.images}</p>
          <p className="stat-label">Images ({c.visualizations} visualizations)</p>
        </div>
      </div>

      <div className="card" style={{ marginBottom: 12 }}>
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
            {data.recent_runs.map((run) => (
              <tr key={run.id}>
                <td>
                  <Link to={`/runs/${run.id}`}>{run.name}</Link>
                </td>
                <td>
                  <span
                    className={`badge badge-${
                      run.status === "completed"
                        ? "success"
                        : run.status === "failed"
                          ? "danger"
                          : "neutral"
                    }`}
                  >
                    {run.status}
                  </span>
                </td>
                <td>{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {data.recent_runs.length === 0 && (
              <tr>
                <td colSpan={3}>No runs yet — train a spaCy pipeline with the coach logger.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {metrics.length > 0 && (
        <div className="card" style={{ marginBottom: 12 }}>
          <h3 style={{ marginTop: 0 }}>Metric series of recent runs</h3>
          <table className="table">
            <thead>
              <tr>
                <th>Run</th>
                <th>Series</th>
                <th>Points</th>
                <th>Latest value</th>
              </tr>
            </thead>
            <tbody>
              {metrics.map((m) => {
                const last = m.points[m.points.length - 1];
                return (
                  <tr key={`${m.run.id}:${m.name}`}>
                    <td>
                      <Link to={`/runs/${m.run.id}`}>{m.run.name}</Link>
                    </td>
                    <td>{m.name}</td>
                    <td>{m.points.length}</td>
                    <td>
                      {last ? Number(last.y.toFixed(4)) : "—"}{" "}
                      <span style={{ color: "var(--color-text-muted)" }}>
                        @ step {last?.x}
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {data.params.length > 0 && hparamKeys.length > 0 && (
        <div className="card">
          <h3 style={{ marginTop: 0 }}>Params comparison</h3>
          <table className="table">
            <thead>
              <tr>
                <th>Run</th>
                {hparamKeys.map((k) => (
                  <th key={k}>{k}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {data.params.map((p) => {
                const flat = flattenHparams(p.hparams, "");
                return (
                  <tr key={p.id}>
                    <td>
                      <Link to={`/runs/${p.id}`}>{p.name}</Link>
                    </td>
                    {hparamKeys.map((k) => (
                      <td key={k}>{flat[k] ?? "—"}</td>
                    ))}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
