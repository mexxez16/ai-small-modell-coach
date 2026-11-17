import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, type DataRun, type DataRunStage, type DataSession, type SourceDoc } from "../api/client";
import HelpBubble from "../components/HelpBubble";

/**
 * Training-data runs: each run is a freely configurable curation pipeline.
 * The editor presents it as six sections (input → preprocessing → output
 * settings → processing → curation → results); on save these map back onto
 * the run's stage list (ssot_import, chunk, ai_annotate, export).
 */

const STATUS_OPTIONS = ["draft", "running", "completed", "failed"];
const EXAMPLE_TYPES = [
  { key: "example", label: "Normale Beispiele (examples)" },
  { key: "negative", label: "Negative Beispiele (nicht-Entität)" },
  { key: "contrastive", label: "Kontrastive Beispiele" },
  { key: "pair", label: "Tripel / Paare (subj—pred—obj)" },
];

type SortKey = "name" | "status" | "session" | "updated_at";

type RunConfig = {
  sourceId: string;
  useChunk: boolean;
  chunkStrategy: string;
  chunkMaxChars: number;
  chunkOverlap: number;
  chunkLabel: string;
  exampleTypes: string[];
  providerId: string;
  instruction: string;
  aiLabels: string;
  aiLimit: number;
  reviewRequired: boolean;
  reviewPerType: boolean;
};

const DEFAULT_CONFIG: RunConfig = {
  sourceId: "",
  useChunk: false,
  chunkStrategy: "paragraph",
  chunkMaxChars: 600,
  chunkOverlap: 0,
  chunkLabel: "imported-chunk",
  exampleTypes: ["example", "pair"],
  providerId: "",
  instruction: "",
  aiLabels: "",
  aiLimit: 0,
  reviewRequired: true,
  reviewPerType: false,
};

function stagesFromConfig(c: RunConfig): DataRunStage[] {
  const stages: DataRunStage[] = [];
  if (c.sourceId) {
    stages.push({ kind: "ssot_import", config: { source_id: c.sourceId, label_overrides: {} } });
  }
  if (c.useChunk && c.sourceId) {
    stages.push({
      kind: "chunk",
      config: {
        source_id: c.sourceId,
        strategy: c.chunkStrategy,
        max_chars: c.chunkMaxChars,
        overlap: c.chunkOverlap,
        label: c.chunkLabel,
      },
    });
  }
  if (c.providerId) {
    stages.push({
      kind: "ai_annotate",
      config: {
        provider_id: c.providerId,
        instruction: c.instruction,
        labels: c.aiLabels
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
        kinds: c.exampleTypes,
        limit: c.aiLimit,
        review: { required: c.reviewRequired, per_type: c.reviewPerType },
      },
    });
  }
  // Export runs always last; heads follow the chosen example types.
  const heads = EXAMPLE_TYPES.filter((t) => c.exampleTypes.includes(t.key)).map((t) => ({
    name: t.key === "pair" ? "relation" : t.key === "negative" || t.key === "contrastive" ? "relevance" : "type",
    kinds: t.key === "pair" ? ["pair"] : [t.key],
  }));
  stages.push({ kind: "export", config: { heads } });
  return stages;
}

function configFromStages(stages: DataRunStage[]): RunConfig {
  const c: RunConfig = { ...DEFAULT_CONFIG };
  for (const stage of stages) {
    const cfg = stage.config ?? {};
    if (stage.kind === "ssot_import") {
      c.sourceId = String(cfg.source_id ?? "");
    } else if (stage.kind === "chunk") {
      c.useChunk = true;
      c.chunkStrategy = String(cfg.strategy ?? "paragraph");
      c.chunkMaxChars = Number(cfg.max_chars ?? 600);
      c.chunkOverlap = Number(cfg.overlap ?? 0);
      c.chunkLabel = String(cfg.label ?? "imported-chunk");
    } else if (stage.kind === "ai_annotate") {
      c.providerId = String(cfg.provider_id ?? "");
      c.instruction = String(cfg.instruction ?? "");
      c.aiLabels = Array.isArray(cfg.labels) ? cfg.labels.join(", ") : "";
      c.exampleTypes = Array.isArray(cfg.kinds) && cfg.kinds.length ? (cfg.kinds as string[]) : c.exampleTypes;
      c.aiLimit = Number(cfg.limit ?? 0);
      const review = cfg.review as { required?: boolean; per_type?: boolean } | undefined;
      c.reviewRequired = review?.required ?? true;
      c.reviewPerType = review?.per_type ?? false;
    }
  }
  return c;
}

const DEFAULT_PROMPT =
  "Klassifiziere jeden Text in genau eines der erlaubten Labels.\n\nErlaubte Labels:\n{labels}\n\nText:\n{text}\n\nAntworte NUR mit einem JSON-Objekt: {\"label\": \"<label>\", \"reasoning\": \"ein kurzer Satz\"}";

export default function DataRuns() {
  const { runId } = useParams<{ runId?: string }>();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<DataRun[]>([]);
  const [sessions, setSessions] = useState<DataSession[]>([]);
  const [sources, setSources] = useState<SourceDoc[]>([]);
  const [providers, setProviders] = useState<Array<{ id: string; name: string; kind: string }>>([]);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [sessionFilter, setSessionFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("updated_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [newRunName, setNewRunName] = useState("");
  const [newRunSession, setNewRunSession] = useState("");
  const [cfg, setCfg] = useState<RunConfig>(DEFAULT_CONFIG);
  const set = <K extends keyof RunConfig>(key: K, value: RunConfig[K]) =>
    setCfg((prev) => ({ ...prev, [key]: value }));

  const detail = useMemo(() => runs.find((r) => r.id === runId) ?? null, [runs, runId]);

  const refresh = useCallback(async () => {
    try {
      const [r, s] = await Promise.all([api.listDataRuns(), api.listDataSessions()]);
      setRuns(r);
      setSessions(s);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
    api.listSources().then(setSources).catch(() => {});
    api.listProviders().then((d) => setProviders((d.providers ?? []) as never)).catch(() => {});
  }, [refresh]);

  // Step configs boot from the run's stages; when a run is selected AND the
  // config files know nothing yet, the run becomes the initial template. Any
  // section change (inputs below) then auto-saves into <config>/<step>.json
  // and back into the run stages — two-way, no manual re-entry.
  const stepLoadTimer = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const stepKeys: Array<keyof RunConfig | string> = [
        "sourceId", "chunkStrategy", "chunkMaxChars", "chunkOverlap", "chunkLabel",
        "exampleTypes", "providerId", "aiLabels", "aiLimit", "reviewRequired", "reviewPerType",
      ];
      void stepKeys;
      try {
        const [input, prep, outs, proc, cur] = await Promise.all([
          api.getStepConfig("input"), api.getStepConfig("preprocessing"),
          api.getStepConfig("output_settings"), api.getStepConfig("processing"),
          api.getStepConfig("curation"),
        ]);
        if (cancelled) return;
        const merged: RunConfig = { ...DEFAULT_CONFIG };
        const fileDefaults = {
          ...(input ?? {}),
          ...(outs ?? {}),
          ...(proc ?? {}),
          ...(cur ?? {}),
          ...((prep as Record<string, unknown>) ?? {}),
        } as Record<string, unknown>;
        if (fileDefaults.source_id) merged.sourceId = String(fileDefaults.source_id);
        if (fileDefaults.useChunk !== undefined) merged.useChunk = Boolean(fileDefaults.useChunk);
        if (fileDefaults.strategy) merged.chunkStrategy = String(fileDefaults.strategy);
        if (fileDefaults.max_chars !== undefined) merged.chunkMaxChars = Number(fileDefaults.max_chars);
        if (fileDefaults.overlap !== undefined) merged.chunkOverlap = Number(fileDefaults.overlap);
        if (fileDefaults.label) merged.chunkLabel = String(fileDefaults.label);
        if (Array.isArray(fileDefaults.example_types) && (fileDefaults.example_types as string[]).length) {
          merged.exampleTypes = fileDefaults.example_types as string[];
        }
        if (fileDefaults.provider_id) merged.providerId = String(fileDefaults.provider_id);
        if (fileDefaults.instruction) merged.instruction = String(fileDefaults.instruction);
        if (fileDefaults.labels) merged.aiLabels = Array.isArray(fileDefaults.labels) ? (fileDefaults.labels as string[]).join(", ") : String(fileDefaults.labels);
        if (fileDefaults.limit !== undefined) merged.aiLimit = Number(fileDefaults.limit);
        if (fileDefaults.review_required !== undefined) merged.reviewRequired = Boolean(fileDefaults.review_required);
        if (fileDefaults.review_per_type !== undefined) merged.reviewPerType = Boolean(fileDefaults.review_per_type);
        if (!detail || !Array.isArray(detail.stages)) {
          setCfg(merged);
        } else {
          setCfg({ ...merged, ...configFromStages(detail.stages) });
        }
      } catch {
        if (!detail || !Array.isArray(detail.stages)) setCfg(DEFAULT_CONFIG);
        else setCfg(configFromStages(detail.stages));
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail?.id]);

  // Auto-save: whenever the user edits a section, the belonging step config
  // file is PUT with the snake_case version of those fields (the UI never
  // blocks on this — it just writes through).
  const persistStep = useCallback((step: string, payload: Record<string, unknown>) => {
    if (stepLoadTimer.current) window.clearTimeout(stepLoadTimer.current);
    stepLoadTimer.current = window.setTimeout(() => {
      api.putStepConfig(step, payload).catch(() => {});
    }, 400);
  }, []);

  const updateAndPersist = <K extends keyof RunConfig>(key: K, value: RunConfig[K]) => {
    set(key, value);
    const mapping: Partial<Record<keyof RunConfig, string>> = {
      sourceId: "input",
      useChunk: "preprocessing",
      chunkStrategy: "preprocessing",
      chunkMaxChars: "preprocessing",
      chunkOverlap: "preprocessing",
      chunkLabel: "preprocessing",
      exampleTypes: "output_settings",
      providerId: "processing",
      instruction: "processing",
      aiLabels: "processing",
      aiLimit: "processing",
      reviewRequired: "curation",
      reviewPerType: "curation",
    };
    const step = mapping[key];
    if (!step) return;
    const snake: Record<string, unknown> = {};
    if (key === "sourceId") snake.source_id = value;
    if (key === "useChunk") snake.useChunk = value;
    if (key === "chunkStrategy") snake.strategy = value;
    if (key === "chunkMaxChars") snake.max_chars = value;
    if (key === "chunkOverlap") snake.overlap = value;
    if (key === "chunkLabel") snake.label = value;
    if (key === "exampleTypes") snake.example_types = value;
    if (key === "providerId") snake.provider_id = value;
    if (key === "instruction") snake.instruction = value;
    if (key === "aiLabels") snake.labels = value;
    if (key === "aiLimit") snake.limit = value;
    if (key === "reviewRequired") snake.review_required = value;
    if (key === "reviewPerType") snake.review_per_type = value;
    if (key === "useChunk" || key === "chunkStrategy") {
      persistStep("preprocessing", {
        useChunk: key === "useChunk" ? value : cfg.useChunk,
        strategy: key === "chunkStrategy" ? value : cfg.chunkStrategy,
      });
    } else if (step) {
      persistStep(step, snake);
    }
  };

  const createSampleCase = async () => {
    setBusy(true);
    try {
      const sample = {
        name: "beispiel-case-testlabor.json",
        content: JSON.stringify({
          entities: {
            "messgerät": { bfo_class: "Material Entity", examples: ["Thermometer Typ K im Labor 3", "Kalibriertes Multimeter auf Werkbank 2"], negative_examples: ["die Idee eines Messgeräts"], contrastive_examples: ["Das Demogerät ist ausgeschaltet, bleibt aber ein Messgerät"] },
            "kalibrierung": { bfo_class: "Process", examples: ["Jährliche Kalibrierung des Multimeters", "Kalibrierung vor jedem Einsatz"], negative_examples: ["die Kalibrier-Erinnerung im Kalender"], contrastive_examples: ["Eine geplante, aber noch nicht durchgeführte Kalibrierung"] },
          },
          relations: {
            "misst": { relation_name: "misst", edge_relation_type: "measures", examples: ["das Multimeter misst Spannung", "Thermometer misst Temperatur im Labor"] },
          },
        }, null, 2),
      };
      const doc = await api.createSource(sample.name, sample.content);
      setSources((prev) => [doc as never, ...prev]);
      updateAndPersist("sourceId", doc.id);
      setResult("Beispiel-Quelle angelegt und vorausgewählt.");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const createRun = async () => {
    if (!newRunName.trim()) return;
    setBusy(true);
    try {
      const run = await api.createDataRun(newRunName.trim(), newRunSession || undefined, []);
      setNewRunName("");
      await refresh();
      navigate(`/training-data/runs/${run.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const executeRun = async (id: string) => {
    setBusy(true);
    setResult(null);
    try {
      const run = await api.executeDataRun(id);
      const entries = Object.entries(run.results ?? {}).filter(([k]) => k !== "errors");
      const summary = entries
        .map(([k, v]) => {
          const obj = v as Record<string, unknown>;
          return `${k}: added=${obj.added ?? "—"} heads=${Array.isArray(obj.heads) ? obj.heads.length : "—"}`;
        })
        .join(" · ");
      setResult(summary || `Status: ${run.status}`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const saveConfig = async () => {
    if (!detail) return;
    setBusy(true);
    try {
      await api.updateDataRun(detail.id, { stages: stagesFromConfig(cfg) });
      await refresh();
      setResult("Pipeline-Konfiguration gespeichert.");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const saveAndExecute = async () => {
    if (!detail) return;
    setBusy(true);
    try {
      await api.updateDataRun(detail.id, { stages: stagesFromConfig(cfg) });
      await executeRun(detail.id);
    } finally {
      setBusy(false);
    }
  };

  const deleteRun = async (id: string) => {
    try {
      await api.deleteDataRun(id);
      if (runId === id) navigate("/training-data/runs");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const visible = runs
    .filter((r) => (statusFilter ? r.status === statusFilter : true))
    .filter((r) => (sessionFilter ? r.session_id === sessionFilter : true))
    .filter((r) => (search.trim() ? r.name.toLowerCase().includes(search.trim().toLowerCase()) : true))
    .sort((a, b) => {
      const dir = sortAsc ? 1 : -1;
      const va = sortKey === "session" ? sessions.find((s) => s.id === a.session_id)?.name ?? "" : sortKey === "updated_at" ? a.updated_at : a[sortKey];
      const vb = sortKey === "session" ? sessions.find((s) => s.id === b.session_id)?.name ?? "" : sortKey === "updated_at" ? b.updated_at : b[sortKey];
      return va < vb ? -dir : va > vb ? dir : 0;
    });

  const sortHeader = (key: SortKey, label: string) => (
    <th
      style={{ cursor: "pointer" }}
      onClick={() => {
        if (sortKey === key) setSortAsc((v) => !v);
        else {
          setSortKey(key);
          setSortAsc(true);
        }
      }}
    >
      {label} {sortKey === key ? (sortAsc ? "↑" : "↓") : ""}
    </th>
  );

  const sectionCard = (nr: string, title: string, helpId: string, children: React.ReactNode) => (
    <div className="card" style={{ marginBottom: 14 }}>
      <h3 style={{ marginTop: 0 }}>
        <span style={{ color: "var(--accent, #7c6cf0)" }}>{nr}</span> {title}
        <HelpBubble helpId={helpId} />
      </h3>
      {children}
    </div>
  );

  return (
    <div>
      <div className="page-header">
        <h1>
          Training Data Runs <HelpBubble helpId="data-runs.pipeline" />
        </h1>
        <p>
          Jeder Run ist eine frei konfigurierbare Kuratierungs-Pipeline in sechs
          Abschnitten — von der Datenquelle bis zu exportfähigen Trainingsdaten.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {result && <div className="stat-label">{result}</div>}

      {!runId && (
        <>
          <div className="card" style={{ marginBottom: 16 }}>
            <h3>Neuen Run anlegen</h3>
            <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
              <input
                className="input"
                placeholder="Run-Name"
                value={newRunName}
                onChange={(e) => setNewRunName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void createRun()}
                style={{ width: 260 }}
              />
              <select
                className="select"
                value={newRunSession}
                onChange={(e) => setNewRunSession(e.target.value)}
                aria-label="Session"
              >
                <option value="">— Session zuordnen —</option>
                {sessions.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
              <button className="button button-primary" disabled={busy || !newRunName.trim()} onClick={() => void createRun()}>
                Run anlegen
              </button>
            </div>
          </div>

          <div className="field-row" style={{ marginBottom: 8, flexWrap: "wrap", gap: 8 }}>
            <input
              className="input"
              placeholder="Runs filtern…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              style={{ width: 200 }}
            />
            <select className="select" value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)} aria-label="Status-Filter">
              <option value="">— Status —</option>
              {STATUS_OPTIONS.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
            <select className="select" value={sessionFilter} onChange={(e) => setSessionFilter(e.target.value)} aria-label="Session-Filter">
              <option value="">— alle Sessions —</option>
              {sessions.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          </div>

          <table className="table">
            <thead>
              <tr>
                {sortHeader("name", "Run")}
                {sortHeader("session", "Session")}
                {sortHeader("status", "Status")}
                <th>Stages</th>
                <th>Dataset</th>
                {sortHeader("updated_at", "Aktualisiert")}
                <th />
              </tr>
            </thead>
            <tbody>
              {visible.length === 0 && (
                <tr>
                  <td colSpan={7}>Keine Runs gefunden.</td>
                </tr>
              )}
              {visible.map((run) => (
                <tr key={run.id}>
                  <td>{run.name}</td>
                  <td>{sessions.find((s) => s.id === run.session_id)?.name ?? "—"}</td>
                  <td>
                    <span className="badge badge-neutral">{run.status}</span>
                  </td>
                  <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                    {Array.isArray(run.stages) ? run.stages.map((s) => s.kind).join(" → ") : "—"}
                  </td>
                  <td>{run.dataset_id ? `${run.dataset_id.slice(0, 8)}…` : "—"}</td>
                  <td>{new Date(run.updated_at).toLocaleString()}</td>
                  <td className="field-row" style={{ gap: 6 }}>
                    <Link className="button" to={`/training-data/runs/${run.id}`}>
                      Pipeline
                    </Link>
                    <button className="button button-primary" disabled={busy} onClick={() => void executeRun(run.id)}>
                      Ausführen
                    </button>
                    <button className="button button-ghost" onClick={() => void deleteRun(run.id)}>
                      Löschen
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}

      {detail && (
        <div>
          <div className="field-row" style={{ justifyContent: "space-between", marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>
              {detail.name}{" "}
              <span className="badge badge-neutral">{detail.status}</span>
            </h2>
            <div className="field-row" style={{ gap: 6 }}>
              <button className="button" onClick={() => navigate("/training-data/runs")}>
                ← Zu allen Runs
              </button>
              <button className="button" disabled={busy || !cfg.sourceId} onClick={() => void saveConfig()}>
                Nur speichern
              </button>
              <button className="button button-primary" disabled={busy || !cfg.sourceId} onClick={() => void saveAndExecute()}>
                Speichern + Ausführen
              </button>
            </div>
          </div>

          {sectionCard("1", "Data Input Selection", "pipeline.input", (
            <>
              <p className="stat-label">
                Quelle der Pipeline: strukturierte SSOT-Kopie (entities/relations) oder Textdokument.
                Neue Quellen hochgeladen werden im Workbench-Bereich unter /training-data.
              </p>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
                <select
                  className="select"
                  value={cfg.sourceId}
                  onChange={(e) => updateAndPersist("sourceId", e.target.value)}
                  aria-label="Datenquelle"
                  style={{ width: 320 }}
                >
                  <option value="">— Quelle wählen —</option>
                  {sources.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name} ({(s.size / 1024).toFixed(1)} KB)
                    </option>
                  ))}
                </select>
                <Link className="button" to="/training-data">
                  Quellen verwalten…
                </Link>
              </div>
              <div className="field-row" style={{ marginTop: 8, flexWrap: "wrap", gap: 8, alignItems: "center" }}>
                <button
                  className="button"
                  onClick={() => void createSampleCase()}
                >
                  Beispiel-Quelle anlegen
                </button>
                <span className="stat-label">
                  Legt eine strukturierte Beispieldatei an und wählt sie aus — zum
                  Ausprobieren der kompletten Pipeline.
                </span>
              </div>
              {!cfg.sourceId && (
                <p className="stat-label" style={{ marginTop: 6 }}>
                  Ohne Quelle kann die Pipeline nicht ausgeführt werden.
                </p>
              )}
            </>
          ))}

          {sectionCard("2", "Pre-Processing (Chunking)", "pipeline.preprocessing", (
            <>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
                <label>
                  <input
                    type="checkbox"
                    checked={cfg.useChunk}
                    onChange={(e) => updateAndPersist("useChunk", e.target.checked)}
                    style={{ marginRight: 6 }}
                  />
                  Chunking aktiv
                </label>
                <select className="select" value={cfg.chunkStrategy} onChange={(e) => updateAndPersist("chunkStrategy", e.target.value)} disabled={!cfg.useChunk} aria-label="Strategie">
                  <option value="paragraph">Absatz (Satzgrenzen-Blöcke)</option>
                  <option value="sentence">Einzelne Sätze</option>
                  <option value="window">Gleitendes Fenster</option>
                </select>
                <label>
                  max. Zeichen
                  <input
                    className="input"
                    type="number"
                    min={40}
                    value={cfg.chunkMaxChars}
                    onChange={(e) => updateAndPersist("chunkMaxChars", Number(e.target.value))}
                    disabled={!cfg.useChunk}
                    style={{ width: 80, marginLeft: 6 }}
                  />
                </label>
                <label>
                  Überlappung
                  <input
                    className="input"
                    type="number"
                    min={0}
                    value={cfg.chunkOverlap}
                    onChange={(e) => updateAndPersist("chunkOverlap", Number(e.target.value))}
                    disabled={!cfg.useChunk || cfg.chunkStrategy !== "window"}
                    style={{ width: 70, marginLeft: 6 }}
                  />
                </label>
                <label>
                  Label
                  <input
                    className="input"
                    placeholder="kebab-case"
                    value={cfg.chunkLabel}
                    onChange={(e) => updateAndPersist("chunkLabel", e.target.value)}
                    disabled={!cfg.useChunk}
                    style={{ width: 160, marginLeft: 6, fontFamily: "var(--font-mono)" }}
                  />
                </label>
              </div>
              <p className="stat-label" style={{ marginTop: 6 }}>
                Vorkonfiguriert ist der deterministische Chunker (Absatz/Satz/Fenster) —
                reproduzierbar und ohne LLM-Kosten. LLM-gestützte Zerlegung folgt als Option.
              </p>
            </>
          ))}

          {sectionCard("3", "Data Output Settings (Beispiel-Typen)", "pipeline.output_settings", (
            <>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 12 }}>
                {EXAMPLE_TYPES.map((t) => (
                  <label key={t.key} style={{ display: "flex", gap: 6, alignItems: "center" }}>
                    <input
                      type="checkbox"
                      checked={cfg.exampleTypes.includes(t.key)}
                      onChange={(e) =>
                        set(
                          "exampleTypes",
                          e.target.checked
                            ? [...cfg.exampleTypes, t.key]
                            : cfg.exampleTypes.filter((x) => x !== t.key),
                        )
                      }
                    />
                    {t.label}
                  </label>
                ))}
              </div>
              <p className="stat-label" style={{ marginTop: 6 }}>
                Ausgewählte Typen bestimmen die Export-Heads: type (examples),
                relevance (negative/contrastive), relation (Tripel-Paare mit
                {" <SEP> "}-Trenner).
              </p>
            </>
          ))}

          {sectionCard("4", "Data Processing (KI / Modell)", "pipeline.processing", (
            <>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
                <select className="select" value={cfg.providerId} onChange={(e) => updateAndPersist("providerId", e.target.value)} aria-label="Modell/Provider" style={{ width: 300 }}>
                  <option value="">— kein LLM (nur Import) —</option>
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.kind})
                    </option>
                  ))}
                </select>
                <label>
                  Limit (0 = alle)
                  <input
                    className="input"
                    type="number"
                    min={0}
                    value={cfg.aiLimit}
                    onChange={(e) => updateAndPersist("aiLimit", Number(e.target.value))}
                    disabled={!cfg.providerId}
                    style={{ width: 90, marginLeft: 6 }}
                  />
                </label>
              </div>
              <div style={{ marginTop: 8 }}>
                <b style={{ fontSize: 13 }}>
                  Prompt-Vorlage bearbeiten <HelpBubble helpId="pipeline.prompt" />
                </b>
                <textarea
                  className="input"
                  value={cfg.instruction || DEFAULT_PROMPT}
                  onChange={(e) => updateAndPersist("instruction", e.target.value)}
                  disabled={!cfg.providerId}
                  placeholder={DEFAULT_PROMPT}
                  style={{ width: "100%", minHeight: 110, fontFamily: "var(--font-mono)", fontSize: 12, marginTop: 4 }}
                />
                <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 6 }}>
                  <input
                    className="input"
                    placeholder="Erlaubte Labels (Komma-getrennt, leer = aus Datensatz)"
                    value={cfg.aiLabels}
                    onChange={(e) => updateAndPersist("aiLabels", e.target.value)}
                    disabled={!cfg.providerId}
                    style={{ width: 320 }}
                  />
                  <button
                    className="button button-ghost"
                    onClick={() => updateAndPersist("instruction", "")}
                    title="Prompt-Vorlage auf Standard zurücksetzen"
                  >
                    Vorlage zurücksetzen
                  </button>
                </div>
              </div>
            </>
          ))}

          {sectionCard("5", "Examples Curation (Human Check)", "pipeline.curation", (
            <>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 12 }}>
                <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  <input
                    type="checkbox"
                    checked={cfg.reviewRequired}
                    onChange={(e) => updateAndPersist("reviewRequired", e.target.checked)}
                  />
                  Menschliche Sichtung erforderlich
                </label>
                <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  <input
                    type="checkbox"
                    checked={cfg.reviewPerType}
                    onChange={(e) => updateAndPersist("reviewPerType", e.target.checked)}
                    disabled={!cfg.reviewRequired}
                  />
                  Sichtung einzelnd nach Typ (nicht als Sammelstapel)
                </label>
              </div>
              <p className="stat-label" style={{ marginTop: 6 }}>
                KI-Antworten landen immer als Entwurf (raw) — nichts wird automatisch
                approved. Die Sichtung passiert in der Workbench: einzelne Items oder
                als Bunch pro Beispiel-Typ.
              </p>
            </>
          ))}

          {sectionCard("6", "Data Output", "pipeline.output", (
            <>
              <p className="stat-label">
                Dataset: {detail.dataset_id ? (
                  <>
                    <a href={`/api/datasets/${detail.dataset_id}/export`} download>
                      JSONL herunterladen
                    </a>{" "}
                    ·{" "}
                    <Link to={`/training-data?dataset=${detail.dataset_id}`}>
                      Items kuratieren (Workbench)
                    </Link>
                  </>
                ) : (
                  "wird bei der ersten Ausführung erzeugt"
                )}
              </p>
              {detail.results && Object.keys(detail.results).length > 0 ? (
                <table className="table">
                  <thead>
                    <tr>
                      <th>Stage</th>
                      <th>Kennzahlen</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(detail.results).map(([key, value]) => (
                      <tr key={key}>
                        <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>{key}</td>
                        <td style={{ fontFamily: "var(--font-mono)", fontSize: 11, maxWidth: 520, overflow: "hidden", textOverflow: "ellipsis" }}>
                          {JSON.stringify(value)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : (
                <div className="empty-state">Noch nicht ausgeführt — „Speichern + Ausführen" oben.</div>
              )}
            </>
          ))}
        </div>
      )}
    </div>
  );
}
