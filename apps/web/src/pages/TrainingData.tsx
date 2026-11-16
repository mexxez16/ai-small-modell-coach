import { useCallback, useEffect, useState } from "react";

type DataItem = {
  id: string;
  dataset_id: string;
  text?: string | null;
  subject?: string | null;
  predicate?: string | null;
  object?: string | null;
  label: string;
  kind: string;
  source: string;
  status: string;
  split: string;
  assignee?: string | null;
  notes?: string | null;
  updated_at: string;
};

type Job = {
  id: string;
  dataset_id: string;
  status: string;
  params: { dev_ratio?: number; seed?: number };
  artifacts: Array<{ sha256?: string; note?: string }>;
  stats: { train?: number; dev?: number; eval?: number; total_items?: number; approved_items?: number };
  created_at: string;
};

type HistoryEntry = {
  id: string;
  timestamp: string;
  actor: string;
  field: string;
  old_value: unknown;
  new_value: unknown;
};

const STATUSES = ["raw", "curated", "approved", "rejected"];
const SPLITS = ["unassigned", "train", "dev", "eval"];

export default function TrainingData() {
  const [datasets, setDatasets] = useState<Array<{ id: string; name: string }>>([]);
  const [active, setActive] = useState<string>("");
  const [items, setItems] = useState<DataItem[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [historyFor, setHistoryFor] = useState<DataItem | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [devRatio, setDevRatio] = useState("0.2");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const list = (await (await fetch("/api/datasets")).json()) as Array<{
      id: string;
      name: string;
    }>;
    setDatasets(list);
    if (!active && list.length > 0) setActive(list[0].id);
    if (active) {
      const rows = (await (
        await fetch(`/api/datasets/${active}/items`)
      ).json()) as DataItem[];
      setItems(rows);
      const j = (await (
        await fetch(`/api/datasets/${active}/conversions`)
      ).json()) as Job[];
      setJobs(j);
    }
  }, [active]);

  useEffect(() => {
    refresh().catch((err: Error) => setError(err.message));
  }, [refresh]);

  const bulkApply = async (patch: { status?: string; split?: string }) => {
    if (selected.size === 0) return;
    try {
      for (const id of selected) {
        const item = items.find((i) => i.id === id);
        if (!item) continue;
        await fetch(`/api/datasets/${active}/items`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            dataset_id: active,
            text: item.text,
            label: item.label,
            kind: item.kind,
            status: patch.status ?? item.status,
            split: patch.split ?? item.split,
            notes: item.notes ?? "",
            source: item.source,
            id,
          }),
        });
      }
      setSelected(new Set());
      await refresh();
      setNotice("Bulk update applied.");
    } catch (err) {
      setError(`Bulk update failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  const runConversion = async () => {
    try {
      const res = await fetch(`/api/datasets/${active}/conversions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ dev_ratio: Number(devRatio) || 0.2 }),
      });
      if (!res.ok) throw new Error(`conversion failed (${res.status})`);
      await refresh();
      const job = (await res.json()) as Job;
      setNotice(
        `Conversion complete: ${JSON.stringify(job.stats)} · sha256 ${
          job.artifacts[0]?.sha256?.slice(0, 12) ?? "—"
        }…`,
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const showHistory = async (item: DataItem) => {
    setHistoryFor(item);
    try {
      const rows = (await (
        await fetch(`/api/items/${item.id}/history`)
      ).json()) as HistoryEntry[];
      setHistory(rows);
    } catch {
      setHistory([]);
    }
  };

  const toggleRow = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const labelCounts = items.reduce<Record<string, number>>((acc, item) => {
    acc[item.label] = (acc[item.label] ?? 0) + 1;
    return acc;
  }, {});

  return (
    <div>
      <div className="page-header">
        <h1>Training Data</h1>
        <p>Curation workbench: review states, splits, audit trail, conversion pipeline.</p>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {notice && <div className="stat-label">{notice}</div>}

      {datasets.length === 0 ? (
        <div className="empty-state">
          <h3>No datasets yet</h3>
          <p>
            Create one via <code>POST /api/datasets</code> — the curation workbench lists items,
            review status, split assignment, audit trail and the conversion pipeline per dataset.
          </p>
        </div>
      ) : (
        <>
          <div className="field-row">
            <select
              className="select"
              value={active}
              onChange={(e) => {
                setActive(e.target.value);
                setSelected(new Set());
              }}
              aria-label="Dataset"
            >
              {datasets.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name}
                </option>
              ))}
            </select>
            <span className="stat-label">
              {Object.entries(labelCounts)
                .map(([label, n]) => `${label}: ${n}`)
                .join(" · ") || "no labels yet"}
            </span>
          </div>

          {selected.size > 0 && (
            <div className="field-row">
              <b>{selected.size} selected:</b>
              {STATUSES.map((st) => (
                <button
                  key={st}
                  className="button"
                  onClick={() => void bulkApply({ status: st })}
                >
                  → {st}
                </button>
              ))}
              <span style={{ width: 12 }} />
              {SPLITS.map((sp) => (
                <button
                  key={sp}
                  className="button"
                  onClick={() => void bulkApply({ split: sp })}
                >
                  → {sp}
                </button>
              ))}
            </div>
          )}

          <table className="table">
            <thead>
              <tr>
                <th />
                <th>Text / Triple</th>
                <th>Label</th>
                <th>Kind</th>
                <th>Status</th>
                <th>Split</th>
                <th>Source</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <tr key={item.id}>
                  <td>
                    <input
                      type="checkbox"
                      checked={selected.has(item.id)}
                      onChange={() => toggleRow(item.id)}
                      aria-label={`select item ${item.id}`}
                    />
                  </td>
                  <td style={{ maxWidth: 320, overflow: "hidden", textOverflow: "ellipsis" }}>
                    {item.text ??
                      `${item.subject ?? "?"} — ${item.predicate ?? "?"} → ${item.object ?? "?"}`}
                  </td>
                  <td>{item.label}</td>
                  <td>{item.kind}</td>
                  <td>
                    <span className="badge badge-neutral">{item.status}</span>
                  </td>
                  <td>{item.split}</td>
                  <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>{item.source}</td>
                  <td>
                    <button className="button" onClick={() => void showHistory(item)}>
                      History
                    </button>
                  </td>
                </tr>
              ))}
              {items.length === 0 && (
                <tr>
                  <td colSpan={8}>No items in this dataset yet.</td>
                </tr>
              )}
            </tbody>
          </table>

          <div className="card" style={{ marginTop: 12 }}>
            <h3 style={{ marginTop: 0 }}>Conversion pipeline</h3>
            <p className="stat-label">
              Stratified train/dev split over all items per label (seeded, deterministic),
              then a dataset hash over the full item snapshot.
            </p>
            <div className="field-row">
              <label>
                dev ratio
                <input
                  type="number"
                  step="0.05"
                  min="0"
                  max="0.5"
                  value={devRatio}
                  onChange={(e) => setDevRatio(e.target.value)}
                  style={{ width: 80, marginLeft: 8 }}
                />
              </label>
              <button className="button button-primary" onClick={() => void runConversion()}>
                Run conversion
              </button>
            </div>
            {jobs.length > 0 && (
              <table className="table" style={{ marginTop: 8 }}>
                <thead>
                  <tr>
                    <th>Run at</th>
                    <th>Status</th>
                    <th>train</th>
                    <th>dev</th>
                    <th>sha256</th>
                  </tr>
                </thead>
                <tbody>
                  {jobs.slice(0, 5).map((job) => (
                    <tr key={job.id}>
                      <td>{new Date(job.created_at).toLocaleString()}</td>
                      <td>{job.status}</td>
                      <td>{job.stats.train ?? "—"}</td>
                      <td>{job.stats.dev ?? "—"}</td>
                      <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                        {job.artifacts[0]?.sha256?.slice(0, 20) ?? "—"}…
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {historyFor && (
            <div className="card" style={{ marginTop: 12 }}>
              <h3 style={{ marginTop: 0 }}>
                Audit trail — &ldquo;{historyFor.label}&rdquo; item
              </h3>
              <button className="button" onClick={() => setHistoryFor(null)}>
                Close
              </button>
              <table className="table" style={{ marginTop: 8 }}>
                <thead>
                  <tr>
                    <th>When</th>
                    <th>Actor</th>
                    <th>Field</th>
                    <th>old → new</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((h) => (
                    <tr key={h.id}>
                      <td>{new Date(h.timestamp).toLocaleString()}</td>
                      <td>{h.actor}</td>
                      <td>{h.field}</td>
                      <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                        {JSON.stringify(h.old_value)} → {JSON.stringify(h.new_value)}
                      </td>
                    </tr>
                  ))}
                  {history.length === 0 && (
                    <tr>
                      <td colSpan={4}>No history entries.</td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}
