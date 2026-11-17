import { useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api, Run } from "../api/client";

type TextRow = { text: string; label: string; dataset: string; datasetId: string };

type DashboardData = {
  counts: {
    sessions: number;
    runs_total: number;
    runs_running: number;
    runs_completed: number;
    runs_failed: number;
    runs_stalled?: number;
    datasets: number;
    data_items: number;
    images: number;
    visualizations: number;
  };
  recent_runs: Run[];
  live_status?: Record<string, string>;
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
  const [texts, setTexts] = useState<TextRow[]>([]);
  const [metrics, setMetrics] = useState<
    Array<{ run: Run; name: string; points: Array<{ x: number; y: number }> }>
  >([]);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
    api
      .dashboard()
      .then(async (d) => {
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
        // Texts index: first 10 items across datasets (smaller workspaces only).
        try {
          const ds = (await (await fetch("/api/datasets")).json()) as Array<{ id: string; name: string }>;
          const rows = (await Promise.all(ds.map(async (d) => {
            try {
              const items = (await (await fetch(`/api/datasets/${d.id}/items`)).json()) as Array<Record<string, unknown>>;
              return items.slice(0, 10).map((i) => ({
                text: String(i.text ?? i.subject ?? ""),
                label: String(i.label ?? ""),
                dataset: d.name,
                datasetId: d.id,
              }));
            } catch {
              return [] as TextRow[];
            }
          }))).flat();
          if (!cancelled) setTexts(rows.slice(0, 12));
        } catch {
          /* texts index is best-effort */
        }
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
  };

  // Live view: re-fetch the summary (and run liveness) every 15 s.
  load();
  const timer = window.setInterval(() => {
    if (!cancelled) load();
  }, 15000);
  return () => {
    cancelled = true;
    window.clearInterval(timer);
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
          <p className="stat-label">Runs ({c.runs_running} running · {c.runs_completed} done · {c.runs_failed} failed{c.runs_stalled ? ` · ${c.runs_stalled} stalled` : ""})</p>
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
            {data.recent_runs.map((run) => {
              const shownStatus = data.live_status?.[run.id] ?? run.status;
              return (
              <tr key={run.id}>
                <td>
                  <Link to={`/runs/${run.id}`}>{run.name}</Link>
                </td>
                <td>
                  <span
                    className={`badge badge-${
                      shownStatus === "completed"
                        ? "success"
                        : shownStatus === "failed"
                          ? "danger"
                          : shownStatus === "running"
                            ? "primary"
                            : "neutral"
                    }`}
                  >
                    {shownStatus}
                  </span>
                </td>
                <td>{new Date(run.created_at).toLocaleString()}</td>
              </tr>
              );
            })}
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

      {texts.length > 0 && (
        <div className="card" style={{ marginBottom: 12 }}>
          <h3 style={{ marginTop: 0 }}>Texts index</h3>
          <table className="table">
            <thead>
              <tr>
                <th>Text</th>
                <th>Label</th>
                <th>Dataset</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {texts.map((t, i) => (
                <tr key={i}>
                  <td style={{ maxWidth: 320, overflow: "hidden", textOverflow: "ellipsis" }}>{t.text}</td>
                  <td>{t.label}</td>
                  <td>{t.dataset}</td>
                  <td>
                    <Link className="button button-ghost" to={`/visualize?text=${encodeURIComponent(t.text)}`}>
                      Visualize
                    </Link>
                  </td>
                </tr>
              ))}
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
