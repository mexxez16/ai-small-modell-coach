import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";

import { api, Run } from "../api/client";

type Series = { name: string; points: Array<{ x: number; y: number }> };

const SERIES_COLORS = ["#4a7cff", "#e0564b", "#2fa36b", "#d1952a", "#8a5bd6", "#2aa9b7"];

/** Multi-series line chart with drag-zoom and optional log scale. */
function SeriesChart({
  seriesList,
  logScale,
}: {
  seriesList: Series[];
  logScale: boolean;
}) {
  const w = 720;
  const h = 260;
  const pad = { l: 48, r: 16, t: 16, b: 30 };
  const [zoom, setZoom] = useState<{ x0: number; x1: number } | null>(null);
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

  const { allX0, allX1, allY0, allY1 } = useMemo(() => {
    let xs: number[] = [];
    let ys: number[] = [];
    for (const s of seriesList) {
      for (const p of s.points) {
        xs.push(p.x);
        if (!logScale || p.y > 0) ys.push(p.y);
      }
    }
    if (xs.length === 0) return { allX0: 0, allX1: 1, allY0: 0, allY1: 1 };
    return {
      allX0: Math.min(...xs),
      allX1: Math.max(...xs),
      allY0: logScale ? Math.min(...ys) : Math.min(...ys),
      allY1: Math.max(...ys),
    };
  }, [seriesList, logScale]);

  const x0 = zoom?.x0 ?? allX0;
  const x1 = zoom?.x1 ?? allX1;
  const spanX = Math.max(x1 - x0, 1);
  const yLo = Math.max(allY0, logScale ? Math.max(allY0, 1e-9) : allY0 - (allY1 - allY0) * 0.05);
  const yHi = allY1 === allY0 ? allY1 + 0.1 : allY1;
  const spanY = yHi - yLo;

  const px = (x: number) => pad.l + ((x - x0) / spanX) * (w - pad.l - pad.r);
  const py = (y: number) =>
    h - pad.b - ((logScale ? Math.log10(Math.max(y, 1e-9)) : y) - (logScale ? Math.log10(yLo) : yLo)) /
      (logScale ? Math.log10(yHi) - Math.log10(yLo) : spanY) *
      (h - pad.t - pad.b);

  const stepFromEventX = (clientX: number): number => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return x0;
    const rel = ((clientX - rect.left) / rect.width) * w;
    return Math.round(x0 + ((rel - pad.l) / (w - pad.l - pad.r)) * spanX);
  };

  if (seriesList.every((s) => s.points.length === 0)) return null;

  return (
    <svg
      ref={svgRef}
      viewBox={`0 0 ${w} ${h}`}
      style={{
        width: "100%",
        maxWidth: 900,
        border: "1px solid var(--color-border)",
        borderRadius: 8,
        touchAction: "none",
        cursor: drag ? "col-resize" : "crosshair",
      }}
      role="img"
      aria-label="metric chart"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        const x = stepFromEventX(e.clientX);
        setDrag({ from: x, to: x });
      }}
      onPointerMove={(e) => {
        if (drag) setDrag({ ...drag, to: stepFromEventX(e.clientX) });
      }}
      onPointerUp={() => {
        if (drag && Math.abs(drag.to - drag.from) > 1) {
          setZoom({ x0: Math.min(drag.from, drag.to), x1: Math.max(drag.from, drag.to) });
        }
        setDrag(null);
      }}
      onDoubleClick={() => setZoom(null)}
    >
      {/* grid lines */}
      {[0.25, 0.5, 0.75].map((t) => (
        <line
          key={t}
          x1={pad.l}
          x2={w - pad.r}
          y1={pad.t + t * (h - pad.t - pad.b)}
          y2={pad.t + t * (h - pad.t - pad.b)}
          stroke="var(--color-border, #333)"
          strokeWidth="0.5"
        />
      ))}

      {/* drag selection */}
      {drag && Math.abs(drag.to - drag.from) > 1 && (
        <rect
          x={Math.min(px(drag.from), px(drag.to))}
          y={pad.t}
          width={Math.max(Math.abs(px(drag.to) - px(drag.from)), 1)}
          height={h - pad.t - pad.b}
          fill="var(--color-accent, #4a7cff)"
          opacity={0.15}
        />
      )}

      {seriesList.map((s, i) => {
        const pts = [...s.points]
          .filter((p) => p.x >= x0 && p.x <= x1 && (!logScale || p.y > 0))
          .sort((a, b) => a.x - b.x);
        return (
          <g key={s.name}>
            <polyline
              points={pts.map((p) => `${px(p.x)},${py(p.y)}`).join(" ")}
              fill="none"
              stroke={SERIES_COLORS[i % SERIES_COLORS.length]}
              strokeWidth="1.6"
            />
            {pts.map((p, j) => (
              <circle
                key={j}
                cx={px(p.x)}
                cy={py(p.y)}
                r="2"
                fill={SERIES_COLORS[i % SERIES_COLORS.length]}
              />
            ))}
          </g>
        );
      })}

      <text x={pad.l} y={h - 8} fontSize="10" fill="var(--color-text-muted, #888)">
        step {x0}
      </text>
      <text
        x={w - pad.r}
        y={h - 8}
        fontSize="10"
        fill="var(--color-text-muted, #888)"
        textAnchor="end"
      >
        step {x1}
      </text>
      <text
        x={8}
        y={pad.t + 10}
        fontSize="10"
        fill="var(--color-text-muted, #888)"
      >
        {logScale ? "log" : "linear"} · double-click to reset
      </text>
      <title>{`${seriesList.length} series · ${x0}..${x1}`}</title>
    </svg>
  );
}

export default function RunDetail() {
  const { runId } = useParams<{ runId: string }>();
  const [run, setRun] = useState<Run | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seriesList, setSeriesList] = useState<Series[]>([]);
  const [logScale, setLogScale] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [showTable, setShowTable] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .listRuns()
      .then((runs) => {
        if (alive) setRun(runs.find((r) => r.id === runId) ?? null);
      })
      .catch(() => undefined);
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
    return () => {
      alive = false;
    };
  }, [runId]);

  const visible = seriesList.filter((s) => !hidden.has(s.name));

  return (
    <div>
      <div className="page-header">
        <h1>{run ? run.name : "Run"}</h1>
        <p>
          {run
            ? `${run.status} · created ${new Date(run.created_at).toLocaleString()}`
            : `run ${runId}`}
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      {seriesList.length > 0 && (
        <div className="card" style={{ marginBottom: 12 }}>
          <div className="field-row">
            <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                type="checkbox"
                checked={logScale}
                onChange={(e) => setLogScale(e.target.checked)}
              />
              Log scale
            </label>
            <button className="button" onClick={() => setShowTable(!showTable)}>
              {showTable ? "Show chart" : "Data table"}
            </button>
          </div>
          {showTable ? (
            <table className="table" style={{ marginTop: 12 }}>
              <thead>
                <tr>
                  <th>series</th>
                  <th>step</th>
                  <th>value</th>
                </tr>
              </thead>
              <tbody>
                {seriesList.flatMap((s) =>
                  s.points.map((p, i) => (
                    <tr key={`${s.name}:${i}`}>
                      <td>{s.name}</td>
                      <td>{p.x}</td>
                      <td>{p.y.toPrecision(6)}</td>
                    </tr>
                  )),
                )}
              </tbody>
            </table>
          ) : (
            <SeriesChart seriesList={visible} logScale={logScale} />
          )}
        </div>
      )}

      {seriesList.length === 0 && !error && (
        <div className="empty-state">
          <h3>No metric points yet</h3>
          <p>Metrics ingest into this view once the training run posts them.</p>
        </div>
      )}

      {seriesList.length > 1 && (
        <div className="card" style={{ marginBottom: 12 }}>
          <h3>Series visibility</h3>
          {seriesList.map((series) => (
            <label key={series.name} style={{ display: "flex", gap: 8 }}>
              <input
                type="checkbox"
                checked={!hidden.has(series.name)}
                onChange={(e) => {
                  setHidden((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.delete(series.name);
                    else next.add(series.name);
                    return next;
                  });
                }}
              />
              <span style={{ color: SERIES_COLORS[seriesList.indexOf(series) % SERIES_COLORS.length] }}>
                ▍
              </span>
              {series.name} ({series.points.length} pts)
            </label>
          ))}
        </div>
      )}

      {run && Object.keys((run.hparams ?? {}) as object).length > 0 && (
        <div className="card">
          <h3>Hyperparameters</h3>
          <pre className="stat-label" style={{ whiteSpace: "pre-wrap" }}>
            {Object.keys(run.hparams as object).length > 0
              ? JSON.stringify(run.hparams, null, 2)
              : ""}
          </pre>
        </div>
      )}
    </div>
  );
}
