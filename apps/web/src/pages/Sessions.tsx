import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api, Run, Session } from "../api/client";

/** Latest value per metric series for one run (fetched for comparison). */
async function runMetricSummary(
  runId: string,
): Promise<Record<string, { latest: number; step: number; points: number }>> {
  try {
    const res = await fetch(`/api/runs/${runId}/metrics`);
    if (!res.ok) return {};
    const pts = (await res.json()) as Array<{ name: string; step: number; value: number }>;
    const out: Record<string, { latest: number; step: number; points: number }> = {};
    for (const p of pts) {
      const cur = out[p.name];
      const candidate = { latest: p.value, step: p.step, points: (cur?.points ?? 0) + 1 };
      if (!cur || p.step >= cur.step) out[p.name] = candidate;
      else out[p.name] = { ...cur, points: candidate.points };
    }
    return out;
  } catch {
    return {};
  }
}

export default function Sessions() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [comparisons, setComparisons] = useState<
    Record<string, Record<string, { latest: number; step: number; points: number }>>
  >({});

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

  // Load comparison data when a session gets selected.
  useEffect(() => {
    if (!selected) return;
    const list = runs.filter((r) => r.session_id === selected);
    for (const run of list) {
      if (comparisons[run.id]) continue;
      void runMetricSummary(run.id).then((summary) =>
        setComparisons((prev) => ({ ...prev, [run.id]: summary })),
      );
    }
  }, [selected, runs, comparisons]);

  const createSession = async () => {
    if (!name.trim()) {
      setError("Enter a session name before creating.");
      return;
    }
    try {
      setError(null);
      const created = await api.createSession(name.trim());
      setName("");
      setSelected(created.id);
      await refresh();
      setNotice(`Session "${created.name}" created.`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const assignRun = async (runId: string, sessionId: string) => {
    try {
      await fetch(`/api/runs/${runId}/assign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sessionId }),
      });
      await refresh();
      setNotice("Run assigned.");
    } catch (err) {
      setError(`Assign failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  const unassigned = runs.filter((r) => !r.session_id);
  const runsOf = (sessionId: string) => runs.filter((r) => r.session_id === sessionId);
  const selectedSession = sessions.find((s) => s.id === selected) ?? null;

  // Distinct metric names across the runs of the selected session.
  const metricNames =
    selected == null
      ? []
      : [
          ...new Set(
            runsOf(selected).flatMap((r) => Object.keys(comparisons[r.id] ?? {})),
          ),
        ].sort();

  return (
    <div>
      <div className="page-header">
        <h1>Sessions</h1>
        <p>Sessions group and compare training runs. Drag a run onto a session to assign it.</p>
      </div>

      <div className="field-row">
        <input
          className="text-input"
          placeholder="New session name"
          value={name}
          aria-label="New session name"
          onChange={(event) => {
            setName(event.target.value);
            if (error) setError(null);
          }}
          onKeyDown={(event) => event.key === "Enter" && void createSession()}
        />
        <button
          className="button button-primary"
          onClick={() => void createSession()}
          disabled={!name.trim()}
        >
          Create session
        </button>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {notice && <div className="stat-label">{notice}</div>}

      <div
        className="card"
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver("unassigned");
        }}
        onDragLeave={() => setDragOver(null)}
        onDrop={() => {
          setDragOver(null);
          setDragging(null);
        }}
        style={
          dragOver === "unassigned"
            ? { outline: "2px dashed var(--color-accent)" }
            : undefined
        }
      >
        <h3 style={{ marginTop: 0 }}>Unassigned runs ({unassigned.length})</h3>
        {unassigned.length === 0 ? (
          <p className="stat-label">All runs belong to a session.</p>
        ) : (
          <ul>
            {unassigned.map((run) => (
              <li
                key={run.id}
                draggable
                onDragStart={() => setDragging(run.id)}
                onDragEnd={() => setDragging(null)}
                style={{ cursor: "grab" }}
              >
                <Link to={`/runs/${run.id}`}>{run.name}</Link>{" "}
                <span className="badge badge-neutral">{run.status}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

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
              <tr
                key={session.id}
                onClick={() => setSelected(session.id)}
                style={{
                  cursor: "pointer",
                  outline:
                    dragOver === session.id
                      ? "2px dashed var(--color-accent)"
                      : undefined,
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDragOver(session.id);
                }}
                onDragLeave={() => setDragOver(null)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDragOver(null);
                  if (dragging) void assignRun(dragging, session.id);
                  setDragging(null);
                }}
              >
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

      {selectedSession && (
        <div className="card">
          <h3 style={{ marginTop: 0 }}>
            Runs in &ldquo;{selectedSession.name}&rdquo;
          </h3>
          {runsOf(selectedSession.id).length === 0 ? (
            <p className="stat-label">
              Drag runs from the unassigned list above onto this session.
            </p>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Run</th>
                  <th>Status</th>
                  {metricNames.map((m) => (
                    <th key={m}>{m} (latest)</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {runsOf(selectedSession.id).map((run) => {
                  const summary = comparisons[run.id] ?? {};
                  return (
                    <tr key={run.id}>
                      <td>
                        <Link to={`/runs/${run.id}`}>{run.name}</Link>
                      </td>
                      <td>
                        <span className="badge badge-neutral">{run.status}</span>
                      </td>
                      {metricNames.map((m) => (
                        <td key={m}>
                          {summary[m] ? Number(summary[m].latest.toFixed(4)) : "—"}
                        </td>
                      ))}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}
