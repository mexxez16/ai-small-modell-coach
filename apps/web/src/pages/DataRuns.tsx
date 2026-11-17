import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, type DataRun, type DataRunStage, type DataSession, type SourceDoc } from "../api/client";
import HelpBubble from "../components/HelpBubble";
import ReviewDeck from "../components/ReviewDeck";

/**
 * Training-data runs: each run is a freely configurable curation pipeline.
 * The editor presents it as six sections (input → preprocessing → output
 * settings → processing → curation → results); on save these map back onto
 * the run's stage list (structured_import, chunk, ai_annotate, export).
 */

/*** Curation block of a data-run: becomes visible once the pipeline produced
 * items; supports the ReviewDeck (per-item) and, grouped by example type,
 * bulk decisions. ***/
function ReviewCurationBlock({
  runItems,
  hasDataset,
  onDecide,
  onEditLabel,
}: {
  runItems: Array<{
    id: string; text?: string | null; subject?: string | null;
    predicate?: string | null; object?: string | null;
    label: string; status: string; source: string;
  }>;
  hasDataset: boolean;
  onDecide: (itemId: string, action: "approved" | "rejected") => Promise<void>;
  onEditLabel: (itemId: string, label: string) => Promise<void>;
}) {
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(10);
  const [reviewMode, setReviewMode] = useState<"deck" | "both" | "table">("both");

  if (!hasDataset) {
    return (
      <div className="empty-state">
        Curation erscheint hier, sobald die Pipeline ausgeführt wurde.
      </div>
    );
  }

  const raw = runItems.filter((i) => i.status === "raw");
  const pageCount = Math.max(1, Math.ceil(raw.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  const rows = raw.slice(safePage * pageSize, (safePage + 1) * pageSize);
  const labels = [...new Set(raw.map((i) => i.label))];

  return (
    <div>
      <div className="field-row" style={{ flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <span className="stat-label">
          Zur Sichtung: {raw.length} von {runItems.length} Items (raw)
        </span>
        <div className="field-row" style={{ gap: 4 }}>
          {(["deck", "both", "table"] as const).map((mode) => (
            <button
              key={mode}
              className={`button ${reviewMode === mode ? "button-primary" : "button-ghost"}`}
              disabled={raw.length === 0}
              onClick={() => setReviewMode(mode)}
            >
              {mode === "deck" ? "Karten-Review" : mode === "both" ? "Karte + Tabelle" : "Tabellen-Review"}
            </button>
          ))}
        </div>
        <select
          className="select"
          value={pageSize}
          onChange={(e) => {
            setPageSize(Number(e.target.value));
            setPage(0);
          }}
          aria-label="Einträge pro Seite"
          title="Einträge pro Seite"
        >
          <option value={10}>10</option>
          <option value={20}>20</option>
          <option value={50}>50</option>
        </select>
        <span className="stat-label">
          Seite {safePage + 1}/{pageCount}
        </span>
        <button className="button button-ghost" disabled={safePage === 0} onClick={() => setPage(safePage - 1)}>
          ←
        </button>
        <button
          className="button button-ghost"
          disabled={safePage >= pageCount - 1}
          onClick={() => setPage(safePage + 1)}
        >
          →
        </button>
      </div>

      {(reviewMode === "deck" || reviewMode === "both") && (
        <ReviewDeck
          items={raw as never}
          onClose={() => setReviewMode("table")}
          onDecide={onDecide}
          onEditLabel={onEditLabel}
        />
      )}

      {(reviewMode === "both" || reviewMode === "table") && (
      <table className="table" style={{ marginTop: 8 }}>
        <thead>
          <tr>
            <th>Inhalt</th>
            <th>Label</th>
            <th style={{ width: 210 }}>Aktionen</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((item) => (
            <tr key={item.id}>
              <td style={{ maxWidth: 380, overflow: "hidden", textOverflow: "ellipsis" }}>
                {item.text ?? `${item.subject ?? "?"} — ${item.predicate ?? "?"} → ${item.object ?? "?"}`}
              </td>
              <td>
                <span className="badge badge-neutral">{item.label}</span>
              </td>
              <td>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
                  <button className="button button-primary" onClick={() => void onDecide(item.id, "approved")}>
                    Genehmigen
                  </button>
                  <button className="button" onClick={() => void onDecide(item.id, "rejected")}>
                    Verwerfen
                  </button>
                </div>
              </td>
            </tr>
          ))}
          {rows.length === 0 && (
            <tr>
              <td colSpan={3}>Keine offenen Items.</td>
            </tr>
          )}
        </tbody>
      </table>
      )}

      <div style={{ marginTop: 12 }}>
        <b>Sammel-Aktionen pro Label</b>
        {labels.map((label) => (
          <div key={label} className="field-row" style={{ gap: 8, marginTop: 4, display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
            <div className="field-row" style={{ gap: 8 }}>
              <span className="stat-label" style={{ minWidth: 140 }}>
                {label}: {raw.filter((i) => i.label === label).length} offen
              </span>
              <button
                className="button button-primary"
                onClick={() =>
                  void Promise.all(raw.filter((i) => i.label === label).map((i) => onDecide(i.id, "approved")))
                }
              >
                Genehmigen
              </button>
              <button
                className="button"
                onClick={() =>
                  void Promise.all(raw.filter((i) => i.label === label).map((i) => onDecide(i.id, "rejected")))
                }
              >
                Verwerfen
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

const STATUS_OPTIONS = ["draft", "running", "completed", "failed"];

/*** Config management for one pipeline step: the step's config file is
 * shown and hand-editable ("Config anzeigen"); picking applies its values to
 * the section inputs; saving writes the current UI state into the file. ***/
function StepConfigControls({
  step,
  payload,
  onApply,
}: {
  step: string;
  /** current section values (snake_case) — keeps the shown JSON live */
  payload?: Record<string, unknown>;
  onApply: (values: Record<string, unknown>) => void;
}) {
  const [show, setShow] = useState(false);
  const [configs, setConfigs] = useState<string[]>([]);
  const [text, setText] = useState("");
  const [msg, setMsg] = useState("");
  const [variantName, setVariantName] = useState("");
  /** true while the user is hand-editing — live overwrite paused then */
  const [dirty, setDirty] = useState(false);

  // Live sync: whenever the section values change (any checkbox/input) and
  // the config view is open and not hand-edited, re-read the file write the
  // view mirrors the actual config state.
  useEffect(() => {
    if (!show || dirty) return;
    // small delay: the section's PUT debounces at 400 ms — refetch after it
    const timer = window.setTimeout(() => {
      fetch(`/api/config/${encodeURIComponent(step)}`)
        .then((r) => r.json() as Promise<Record<string, unknown>>)
        .then((data) => setText(JSON.stringify(data, null, 2)))
        .catch(() => {});
    }, 650);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, show, dirty, JSON.stringify(payload ?? {})]);


  const reloadList = async () => {
    try {
      const res = await fetch(`/api/config/${encodeURIComponent(step)}/list`);
      const data = (await res.json()) as { configs?: string[] };
      setConfigs(data.configs ?? []);
    } catch {
      setConfigs([]);
    }
  };

  const parseText = (): Record<string, unknown> | null => {
    try {
      const parsed = JSON.parse(text || "{}") as Record<string, unknown>;
      return parsed;
    } catch (err) {
      setMsg(`Ungültiges JSON: ${err instanceof Error ? err.message : err}`);
      return null;
    }
  };

  const saveActive = async () => {
    const parsed = parseText();
    if (parsed === null) return;
    const res = await fetch(`/api/config/${encodeURIComponent(step)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(parsed),
    });
    if (res.ok) {
      onApply(parsed);
      setDirty(false);
      setMsg("Config gespeichert — Abschnitt aktualisiert.");
    } else {
      setMsg(`Speichern fehlgeschlagen (${res.status}).`);
    }
  };

  return (
    <div style={{ marginBottom: 8, borderBottom: "1px solid var(--color-border)", paddingBottom: 8 }}>
      <div className="field-row" style={{ flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <select
          className="select"
          defaultValue=""
          onChange={async (e) => {
            const name = e.target.value;
            if (!name) return;
            const res = await fetch(
              `/api/config/${encodeURIComponent(step)}/${encodeURIComponent(name)}`,
            );
            const data = (await res.json()) as Record<string, unknown>;
            // Show the chosen file for hand-editing; applying happens via Speichern.
            setText(JSON.stringify(data, null, 2));
            setDirty(false);
            setShow(true);
            setMsg(`Config "${name}" geladen — Speichern wendet sie an.`);
          }}
          aria-label="Config-Variante wählen"
        >
          <option value="">— Config-Variante laden —</option>
          {configs.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
        <button
          className="button"
          onClick={() => setShow((v) => !v)}
          title="Config-Datei dieses Schritts anzeigen und von Hand bearbeiten"
        >
          {show ? "▲ Config verbergen" : "Config anzeigen"}
        </button>
      </div>
      {show && (
        <div style={{ marginTop: 6 }}>
          <textarea
            className="input"
            value={text}
            onChange={(e) => {
              setDirty(true);
              setText(e.target.value);
            }}
            rows={Math.min(14, Math.max(5, text.split("\n").length))}
            style={{ width: "100%", fontFamily: "var(--font-mono)", fontSize: 12.5 }}
            spellCheck={false}
          />
          <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 6, alignItems: "center" }}>
            <button className="button button-primary" onClick={() => void saveActive()}>
              Speichern + anwenden
            </button>
            <input
              className="input"
              placeholder="Als Variante speichern unter…"
              value={variantName}
              onChange={(e) => setVariantName(e.target.value)}
              style={{ width: 200 }}
            />
            <button
              className="button"
              disabled={!variantName.trim()}
              onClick={() =>
                void (async () => {
                  const parsed = parseText();
                  if (parsed === null) return;
                  const res = await fetch(
                    `/api/config/${encodeURIComponent(step)}/${encodeURIComponent(variantName.trim())}`,
                    {
                      method: "PUT",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify(parsed),
                    },
                  );
                  if (res.ok) {
                    setMsg(`Variante "${variantName.trim()}" gespeichert.`);
                    setVariantName("");
                    await reloadList();
                  }
                })()
              }
            >
              Variante speichern
            </button>
            <button
              className="button button-ghost"
              onClick={() => {
                const parsed = parseText();
                if (parsed !== null) {
                  onApply(parsed);
                  setMsg("Werte in den Abschnitt übernommen.");
                }
              }}
            >
              Ohne Speichern anwenden
            </button>
          </div>
        </div>
      )}
      {msg && (
        <div className="stat-label" style={{ marginTop: 4 }}>
          {msg}
        </div>
      )}
    </div>
  );
}
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
  modelOverride: string;
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
  modelOverride: "",
  reviewRequired: true,
  reviewPerType: false,
};

function stagesFromConfig(c: RunConfig): DataRunStage[] {
  const stages: DataRunStage[] = [];
  if (c.sourceId) {
    stages.push({ kind: "structured_import", config: { source_id: c.sourceId, label_overrides: {} } });
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
        model: c.modelOverride || undefined,
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
    if (stage.kind === "structured_import" || stage.kind === "ssot_import") {
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

  // Items of the run's dataset (curation view + status counts).
  const [runItems, setRunItems] = useState<Array<{
    id: string; text?: string | null; subject?: string | null;
    predicate?: string | null; object?: string | null;
    label: string; status: string; source: string;
  }>>([]);
  useEffect(() => {
    if (!detail?.dataset_id) return;
    fetch(`/api/datasets/${detail.dataset_id}/items`)
      .then((r) => r.json())
      .then((rows: Array<{ id: string; text?: string; subject?: string; predicate?: string; object?: string; label: string; status: string; source: string }>) => setRunItems(rows))
      .catch(() => {});
  }, [detail?.dataset_id, detail?.updated_at, detail?.status]);

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
        if (fileDefaults.model) merged.modelOverride = String(fileDefaults.model);
        if (fileDefaults.modelOverride) merged.modelOverride = String(fileDefaults.modelOverride);
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
      modelOverride: "processing",
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
    if (key === "modelOverride") snake.model = value;
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
            "messgerät": { class: "Material Entity", examples: ["Thermometer Typ K im Labor 3", "Kalibriertes Multimeter auf Werkbank 2"], negative_examples: ["die Idee eines Messgeräts"], contrastive_examples: ["Das Demogerät ist ausgeschaltet, bleibt aber ein Messgerät"] },
            "kalibrierung": { class: "Process", examples: ["Jährliche Kalibrierung des Multimeters", "Kalibrierung vor jedem Einsatz"], negative_examples: ["die Kalibrier-Erinnerung im Kalender"], contrastive_examples: ["Eine geplante, aber noch nicht durchgeführte Kalibrierung"] },
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
      const errors = (run.results?.["errors"] as string[] | undefined) ?? [];
      if (run.status === "failed" || errors.length > 0) {
        setError(
          `Run fehlgeschlagen (${errors.length} Stage-Fehler) — Details im Run-Log unten: ` +
            errors.slice(0, 3).join(" | "),
        );
      } else {
        const summary = entries
          .map(([k, v]) => {
            const obj = v as Record<string, unknown>;
            return `${k}: added=${obj.added ?? "—"} heads=${Array.isArray(obj.heads) ? obj.heads.length : "—"}`;
          })
          .join(" · ");
        setResult(`Run abgeschlossen: ${summary || run.status}`);
      }
      await refresh();
    } catch (err) {
      setError(`Run-Execution fehlgeschlagen: ${err instanceof Error ? err.message : String(err)}`);
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

  /** Current section values in the config-file (snake_case) shape. */
  const stepPayload = (step: string): Record<string, unknown> => {
    switch (step) {
      case "input":
        return { source_id: cfg.sourceId };
      case "preprocessing":
        return {
          strategy: cfg.chunkStrategy,
          max_chars: cfg.chunkMaxChars,
          overlap: cfg.chunkOverlap,
          label: cfg.chunkLabel,
        };
      case "output_settings":
        return { example_types: cfg.exampleTypes };
      case "processing":
        return {
          provider_id: cfg.providerId,
          instruction: cfg.instruction,
          labels: cfg.aiLabels.split(",").map((s) => s.trim()).filter(Boolean),
          limit: cfg.aiLimit,
          model: cfg.modelOverride,
        };
      case "curation":
        return { review_required: cfg.reviewRequired, review_per_type: cfg.reviewPerType };
      default:
        return {};
    }
  };

  /** Decision from the curation deck: write the new status back. */
  const setItemStatus = async (itemId: string, status: string) => {
    const item = runItems.find((i) => i.id === itemId);
    if (!item || !detail?.dataset_id) return;
    setRunItems((prev) => prev.map((i) => (i.id === itemId ? { ...i, status } : i)));
    await fetch(`/api/datasets/${detail.dataset_id}/items`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...item, status }),
    }).catch(() => {});
  };

  /** Apply a loaded config file to the section inputs (two-way sync). */  const applyStepValues = (values: Record<string, unknown>) => {
    const next = { ...cfg };
    if (values.source_id !== undefined) next.sourceId = String(values.source_id);
    if (values.strategy !== undefined) next.chunkStrategy = String(values.strategy);
    if (values.max_chars !== undefined) next.chunkMaxChars = Number(values.max_chars);
    if (values.overlap !== undefined) next.chunkOverlap = Number(values.overlap);
    if (values.label !== undefined) next.chunkLabel = String(values.label);
    if (values.example_types !== undefined) {
      next.exampleTypes = Array.isArray(values.example_types)
        ? (values.example_types as string[])
        : String(values.example_types)
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean);
    }
    if (values.provider_id !== undefined) next.providerId = String(values.provider_id);
    if (values.instruction !== undefined) next.instruction = String(values.instruction);
    if (values.labels !== undefined) {
      next.aiLabels = Array.isArray(values.labels)
        ? (values.labels as string[]).join(", ")
        : String(values.labels);
    }
    if (values.limit !== undefined) next.aiLimit = Number(values.limit);
    if (values.model !== undefined) next.modelOverride = String(values.model ?? "");
    if (values.review_required !== undefined) next.reviewRequired = Boolean(values.review_required);
    if (values.review_per_type !== undefined) next.reviewPerType = Boolean(values.review_per_type);
    setCfg(next);
  };

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
            <>
              <p className="stat-label">
                Quelle der Pipeline: strukturierte JSON-Datei oder Textdokument.
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
            <StepConfigControls step="input" payload={stepPayload("input")} onApply={applyStepValues} />
            </>
            </>
          ))}

          {sectionCard("2", "Pre-Processing (Chunking)", "pipeline.preprocessing", (
            <>
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
            <StepConfigControls step="preprocessing" payload={stepPayload("preprocessing")} onApply={applyStepValues} />
            </>
            </>
          ))}

          {sectionCard("3", "Data Output Settings (Beispiel-Typen)", "pipeline.output_settings", (
            <>
            <>
              <div className="field-row" style={{ flexWrap: "wrap", gap: 12 }}>
                {EXAMPLE_TYPES.map((t) => (
                  <label key={t.key} style={{ display: "flex", gap: 6, alignItems: "center" }}>
                    <input
                      type="checkbox"
                      checked={cfg.exampleTypes.includes(t.key)}
                      onChange={(e) =>
                        updateAndPersist(
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
            <StepConfigControls step="output_settings" payload={stepPayload("output_settings")} onApply={applyStepValues} />
            </>
            </>
          ))}

          {sectionCard("4", "Data Processing (KI / Modell)", "pipeline.processing", (
            <>
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
            <StepConfigControls step="processing" payload={stepPayload("processing")} onApply={applyStepValues} />
            </>
            </>
          ))}

          {sectionCard("5", "Examples Curation (Human Check)", "pipeline.curation", (
            <>
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
    <ReviewCurationBlock
      runItems={runItems}
      hasDataset={Boolean(detail?.dataset_id)}
      onDecide={(id, action) => setItemStatus(id, action)}
      onEditLabel={async (id, label) => {
        const item = runItems.find((i) => i.id === id);
        if (!item || !detail?.dataset_id) return;
        setRunItems((prev) => prev.map((i) => (i.id === id ? { ...i, label } : i)));
        await fetch(`/api/datasets/${detail.dataset_id}/items`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ ...item, label }),
        }).catch(() => {});
      }}
    />
            <StepConfigControls step="curation" payload={stepPayload("curation")} onApply={applyStepValues} />
            </>
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
