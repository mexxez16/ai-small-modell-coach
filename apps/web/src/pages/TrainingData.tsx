import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import HelpBubble from "../components/HelpBubble";
import ReviewDeck from "../components/ReviewDeck";

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

type SourceDoc = {
  id: string;
  name: string;
  kind: string;
  size: number;
  sha256: string;
  created_at: string;
};

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

  // Pipeline state: sources, up/upload, chunk settings, annotation.
  const [sources, setSources] = useState<SourceDoc[]>([]);
  const [sourceName, setSourceName] = useState("");
  const [sourceContent, setSourceContent] = useState("");
  const [chunkStrategy, setChunkStrategy] = useState("paragraph");
  const [chunkMaxChars, setChunkMaxChars] = useState("600");
  const [chunkOverlap, setChunkOverlap] = useState("0");
  const [chunkLabel, setChunkLabel] = useState("imported-chunk");
  const [providers, setProviders] = useState<
    Array<{ id: string; name: string; kind: string }>
  >([]);
  const [providerId, setProviderId] = useState("");
  const [annotationLabels, setAnnotationLabels] = useState("");
  const [annotationInstruction, setAnnotationInstruction] = useState("");
  const [busy, setBusy] = useState(false);
  const [validation, setValidation] = useState<{
    exportable?: number;
    total_items?: number;
    issues?: Array<{ rule: string; detail: string; severity: string }>;
  } | null>(null);
  const [reviewMode, setReviewMode] = useState(false);

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

  // Validation report refreshes with the dataset switch.
  useEffect(() => {
    if (!active) return;
    setValidation(null);
    fetch(`/api/datasets/${active}/validation`)
      .then((r) => r.json() as Promise<NonNullable<typeof validation>>)
      .then(setValidation)
      .catch(() => {});
  }, [active, items]);

  // Sources + providers for the pipeline panels (loaded once per render of
  // the datasets list; the endpoints are cheap lists).
  useEffect(() => {
    fetch("/api/sources")
      .then((r) => r.json() as Promise<SourceDoc[]>)
      .then(setSources)
      .catch(() => {});
    fetch("/api/settings/providers")
      .then((r) => r.json() as Promise<{ providers?: Array<{ id: string; name: string; kind: string }> }>)
      .then((d) => setProviders(d.providers ?? []))
      .catch(() => {});
  }, []);

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

  const uploadSource = async () => {
    if (!sourceContent.trim()) return;
    setBusy(true);
    try {
      const res = await fetch("/api/sources", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: sourceName || `source-${Date.now()}`, content: sourceContent }),
      });
      if (!res.ok) throw new Error(`upload failed (${res.status})`);
      const doc = (await res.json()) as SourceDoc;
      setSources((prev) => [doc as never, ...prev]);
      setSourceName("");
      setSourceContent("");
      setNotice("Quelle gespeichert — jetzt chunking konfigurieren.");
    } catch (err) {
      setError(`Upload fehlgeschlagen: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const chunkSource = async (docId: string) => {
    setBusy(true);
    try {
      const res = await fetch(`/api/sources/${encodeURIComponent(docId)}/chunks`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          strategy: chunkStrategy,
          max_chars: Number(chunkMaxChars) || 600,
          overlap: Number(chunkOverlap) || 0,
          dataset_id: null,
          label: chunkLabel,
        }),
      });
      if (!res.ok) throw new Error(`chunking failed (${res.status})`);
      const data = (await res.json()) as { created_items: number; dataset_id: string };
      setNotice(
        `${data.created_items} Items aus der Quelle erstellt (${chunkStrategy}). Partition: ${data.dataset_id.slice(0, 8)}…`,
      );
      await refresh();
    } catch (err) {
      setError(`Chunking fehlgeschlagen: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const annotateSelection = async () => {
    if (selected.size === 0) return;
    setBusy(true);
    try {
      const payload = {
        provider_id: providers.find((p) => p.kind === "openai_compatible")?.id,
        item_ids: Array.from(selected),
        instruction: annotationInstruction,
        labels: annotationLabels
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      };
      const res = await fetch("/api/annotations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      if (!res.ok) throw new Error(`annotation failed (${res.status})`);
      const data = (await res.json()) as { annotated: number; failed: number; errors: string[] };
      setNotice(
        `KI-Annotation: ${data.annotated} annotiert, ${data.failed} übersprungen.`,
      );
      await refresh();
    } catch (err) {
      setError(`Annotation fehlgeschlagen: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const updateItem = async (
    itemId: string,
    patch: { status?: string; label?: string },
  ) => {
    const item = items.find((i) => i.id === itemId);
    if (!item) return;
    const res = await fetch(`/api/datasets/${active}/items`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        text: item.text ?? "",
        label: patch.label ?? item.label,
        status: patch.status ?? item.status,
        id: itemId,
      }),
    });
    if (!res.ok) throw new Error(`update failed (${res.status})`);
  };

  const labelCounts = items.reduce<Record<string, number>>((acc, item) => {
    acc[item.label] = (acc[item.label] ?? 0) + 1;
    return acc;
  }, {});

  // Validation-badged review toolbar: Review start, issue summary, DocBin
  // export (requires a running render worker).
  function reviewsBelow() {
    if (!validation) return null;
    const blocking = (validation.issues ?? []).filter((i) => i.severity === "critical");
    return (
      <div className="card" style={{ margin: "12px 0" }}>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8, alignItems: "center" }}>
          <b>
            Review-Fortschritt <HelpBubble helpId="train-data.review" />
          </b>
          <span>
            {validation.exportable ?? 0}/{validation.total_items ?? 0} exportfähig
          </span>
          <button
            className="button button-primary"
            onClick={() => setReviewMode((v) => !v)}
            disabled={items.length === 0}
          >
            {reviewMode ? "Review beenden" : "Review starten"}
          </button>
          <a
            className="button"
            href={`/api/datasets/${active}/export/docbin`}
            download
            title=".spacy-DocBin (benötigt laufenden Render-Worker)"
          >
            Export .spacy <HelpBubble helpId="train-data.export" />
          </a>
        </div>
        <div className="stat-label" style={{ marginTop: 6 }}>
          {blocking.length === 0
            ? "Keine kritischen Schema-Fehler."
            : `${blocking.length} kritische Fehler: ${blocking
                .slice(0, 3)
                .map((i) => i.detail)
                .join(" · ")}`}
        </div>
        {(validation.issues ?? []).length > 0 && (
          <ul style={{ margin: "6px 0 0", paddingLeft: 20 }}>
            {(validation.issues ?? []).slice(0, 5).map((issue, idx) => (
              <li key={idx} className={`stat-label ${issue.severity === "critical" ? "" : "muted"}`}>
                {issue.severity === "critical" ? "[blockiert]" : "[Warnung]"} {issue.rule}: {issue.detail}
              </li>
            ))}
          </ul>
        )}
        {reviewMode && (
          <ReviewDeck
            items={items as never}
            onClose={() => setReviewMode(false)}
            onDecide={async (itemId, action) => {
              await updateItem(itemId, { status: action });
            }}
            onEditLabel={async (itemId, label) => {
              await updateItem(itemId, { label });
            }}
          />
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="page-header">
        <h1>Training Data</h1>
        <p>Curation workbench: review states, splits, audit trail, conversion pipeline.</p>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {notice && <div className="stat-label">{notice}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>
          Quellen & Pipeline-Start <HelpBubble helpId="train-data.sources" />
        </h3>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
          <input
            className="input"
            placeholder="Quellname, z.B. BFO_SSOT.txt"
            value={sourceName}
            onChange={(e) => setSourceName(e.target.value)}
            style={{ width: 220 }}
          />
          <textarea
            className="input"
            placeholder="Dokument hier einfügen — oder Datei unten laden (Text, Markdown, JSON, SSOT-Export)"
            value={sourceContent}
            onChange={(e) => setSourceContent(e.target.value)}
            style={{ width: "100%", minHeight: 80 }}
          />
          <label className="button">
            Datei laden
            <input
              type="file"
              accept=".txt,.md,.json,.jsonl,text/*"
              style={{ display: "none" }}
              onChange={async (e) => {
                const file = e.target.files?.[0];
                if (!file) return;
                const text = await file.text();
                setSourceName((prev) => prev || file.name);
                setSourceContent(text);
              }}
            />
          </label>
          <button
            className="button button-primary"
            disabled={busy || !sourceContent.trim()}
            onClick={() => void uploadSource()}
          >
            Quelle speichern
          </button>
        </div>

        {sources.length > 0 && (
          <table className="table" style={{ marginTop: 8 }}>
            <thead>
              <tr>
                <th>Name</th>
                <th>Größe</th>
                <th>sha256</th>
                <th>
                  Chunking <HelpBubble helpId="train-data.chunking" />
                </th>
                <th />
              </tr>
            </thead>
            <tbody>
              {sources.map((doc) => (
                <tr key={doc.id}>
                  <td>{doc.name}</td>
                  <td>{(doc.size / 1024).toFixed(1)} KB</td>
                  <td
                    title={doc.sha256}
                    style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}
                  >
                    {doc.sha256.slice(0, 12)}…
                  </td>
                  <td>
                    <div className="field-row" style={{ gap: 6 }}>
                      <select
                        className="select"
                        value={chunkStrategy}
                        onChange={(e) => setChunkStrategy(e.target.value)}
                        aria-label="Chunking-Strategie"
                        title="Chunking-Strategie"
                      >
                        <option value="paragraph">Absatz</option>
                        <option value="sentence">Satz</option>
                        <option value="window">Fenster</option>
                      </select>
                      <input
                        className="input"
                        type="number"
                        min={40}
                        value={chunkMaxChars}
                        onChange={(e) => setChunkMaxChars(e.target.value)}
                        title="max. Zeichen pro Chunk"
                        style={{ width: 70 }}
                      />
                      <input
                        className="input"
                        type="number"
                        min={0}
                        value={chunkOverlap}
                        onChange={(e) => setChunkOverlap(e.target.value)}
                        title="Überlappung (nur Fenster)"
                        style={{ width: 60 }}
                      />
                      <input
                        className="input"
                        placeholder="label (kebab-case)"
                        value={chunkLabel}
                        onChange={(e) => setChunkLabel(e.target.value)}
                        style={{ width: 150 }}
                      />
                      <button
                        className="button"
                        disabled={busy}
                        onClick={() => void chunkSource(doc.id)}
                      >
                        Chunking starten
                      </button>
                    </div>
                  </td>
                  <td>
                    <button
                      className="button button-ghost"
                      onClick={async () => {
                        await fetch(`/api/sources/${doc.id}`, { method: "DELETE" }).catch(
                          () => {},
                        );
                        setSources((prev) => prev.filter((s) => s.id !== doc.id));
                      }}
                    >
                      Löschen
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

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

          {reviewsBelow()}

          {selected.size > 0 && (
            <div className="card" style={{ margin: "12px 0" }}>              <h3 style={{ marginTop: 0 }}>
                Auswahl: {selected.size} Items <HelpBubble helpId="train-data.annotation" />
              </h3>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
                <select
                  className="select"
                  value={providerId}
                  onChange={(e) => setProviderId(e.target.value)}
                  aria-label="Annotation-Provider"
                  title="Annotation-Provider"
                >
                  <option value="">
                    {providers.find((p) => p.kind === "openai_compatible")
                      ? " — Provider wählen — "
                      : " — erst in Settings einen OpenAI-kompatiblen Provider anlegen — "}
                  </option>
                  {providers
                    .filter((p) => p.kind === "openai_compatible")
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                </select>
                <input
                  className="input"
                  placeholder="erlaubte Labels (Komma-getrennt, leer = aus Datensatz)"
                  value={annotationLabels}
                  onChange={(e) => setAnnotationLabels(e.target.value)}
                  style={{ width: 280 }}
                />
                <input
                  className="input"
                  placeholder="Instruktion für das Modell (optional)"
                  value={annotationInstruction}
                  onChange={(e) => setAnnotationInstruction(e.target.value)}
                  style={{ width: 280 }}
                />
                <button
                  className="button button-primary"
                  disabled={busy || !providerId}
                  onClick={() => void annotateSelection()}
                >
                  KI-Annotation starten
                </button>
              </div>
              {STATUSES.map((st) => (
                <button
                  key={st}
                  className="button"
                  onClick={() => void bulkApply({ status: st })}
                >
                  → {st}
                </button>
              ))}
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
                    <Link
                      className="button button-ghost"
                      to={`/visualize?text=${encodeURIComponent(
                        item.text ?? `${item.subject ?? ""} ${item.predicate ?? ""} ${item.object ?? ""}`,
                      )}`}
                    >
                      Visualize
                    </Link>
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
            <h3 style={{ marginTop: 0 }}>
              Konvertierung & Export <HelpBubble helpId="train-data.export" />
            </h3>
            <p className="stat-label">
              Stratified train/dev split (seeded, per label), dataset hash, spaCy-fähiger
              JSONL-Export — nur curated + approved fließen in den Export.
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
              <a
                className="button"
                href={`/api/datasets/${active}/export?status=approved`}
                download
                title="JSONL-Export (nur approved Items)"
              >
                Export JSONL
              </a>
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
