import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";

type Series = { name: string; points: Array<{ x: number; y: number }> };

/** Line chart for one metric series (pure SVG, no external chart lib). */
function MetricChart({ series }: { series: Series }) {
  if (series.points.length === 0) return null;
  const w = 640;
  const h = 220;
  const pad = 36;
  const xs = series.points.map((p) => p.x);
  const ys = series.points.map((p) => p.y);
  const x0 = Math.min(...xs);
  const x1 = Math.max(...xs);
  const y0 = Math.min(...ys);
  const y1 = Math.max(...ys);
  const spanX = Math.max(x1 - x0, 1);
  const spanY = Math.max(y1 - y0, 0.0001);
  const px = (x: number) => pad + ((x - x0) / spanX) * (w - 2 * pad);
  const py = (y: number) => h - pad - ((y - y0) / spanY) * (h - 2 * pad);
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      style={{
        width: "100%",
        maxWidth: 640,
        border: "1px solid var(--color-border)",
        borderRadius: 8,
      }}
      role="img"
      aria-label={`metric chart ${series.name}`}
    >
      <polyline
        points={series.points.map((p) => `${px(p.x)},${py(p.y)}`).join(" ")}
        fill="none"
        stroke="var(--color-accent, #4a7cff)"
        strokeWidth="1.6"
      />
      {series.points.map((p, i) => (
        <circle key={i} cx={px(p.x)} cy={py(p.y)} r="1.8" fill="var(--color-accent, #4a7cff)" />
      ))}
      <title>{`${series.name}: ${series.points.length} points`}</title>
      <text x={pad} y={h - 6} fontSize="10" fill="var(--color-text-muted, #888)">
        step {x0}
      </text>
      <text x={w - pad} y={h - 6} fontSize="10" fill="var(--color-text-muted, #888)" textAnchor="end">
        step {x1}
      </text>
    </svg>
  );
}

export default function RunDetail() {
  const { runId } = useParams<{ runId: string }>();
  const [error, setError] = useState<string | null>(null);
  const [seriesList, setSeriesList] = useState<Series[]>([]);
  const [hidden, setHidden] = useState<Set<string>>(new Set());

  useEffect(() => {
    let alive = true;
    fetch(`/api/runs/${runId}/metrics`)
      .then((r) => {
        if (!r.ok) throw new Error(`metrics fetch failed (${r.status})`);
        return r.json();
      })
      .then((metrics: Array<{ name: string; step: number; value: number }>) => {
        if (!alive) return;
        const byName = new Map<string, Array<[number, number]>>();
        for (const m of metrics) {
          const bucket = byName.get(m.name) ?? [];
          bucket.push([m.step, m.value]);
          byName.set(m.name, bucket);
        }
        setSeriesList(
          [...byName.entries()].map(([name, pts]) => ({
            name,
            points: pts.map(([x, y]) => ({ x, y })),
          })),
        );
      })
      .catch((err: Error) => setError(err.message));
    return () => undefined;
  }, [runId]);

  return (
    <div>
      <div className="page-header">
        <h1>Run detail</h1>
        <p>Recorded metric series for run &quot;{seriesList.length}&quot; metric(s).</p>
      </div>
      {seriesList.length === 0 && (
        <div className="empty-state">
          <h3>{error ? "Could not load metrics" : "No metric points yet"}</h3>
          <p>
            {error ??
              "Metrics ingest into this view once the training run posts them."}
          </p>
        </div>
      )}
      {seriesList.map((series) => {
        const isHidden = hidden.has(series.name);
        return (
          <div key={series.name} className="card" style={{ marginBottom: 12 }}>
            <label>
              <input
                type="checkbox"
                checked={isHidden}
                onChange={(e) => {
                  setHidden((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.add(series.name);
                    else next.delete(series.name);
                    return next;
                  });
                }}
              />{" "}
              <b>{series.name}</b> — suppressed in the chart list preview
            </label>
          </div>
        );
      })}
      <div className="card">
        {seriesList.map((series) => (
          <MetricChart key={series.name} series={series} />
        ))}
      </div>
    </div>
  );
}
