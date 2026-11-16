import DOMPurify from "dompurify";
import { useCallback, useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";

import { api, RenderWorker, Run, Visualization } from "../api/client";

const STYLES = ["ent", "dep", "span"] as const;
type Style = (typeof STYLES)[number];

const SANITIZE_PROFILE = { USE_PROFILES: { svg: true, svgFilters: true, html: true } };

export default function Visualize() {
  const [searchParams] = useSearchParams();
  const [text, setText] = useState(
    searchParams.get("text") ??
      "Apple is looking at buying U.K. startup for $1 billion.",
  );
  const [style, setStyle] = useState<Style>("ent");
  const [ents, setEnts] = useState("");
  const [compact, setCompact] = useState(false);
  const [distance, setDistance] = useState("120");
  const [bg, setBg] = useState("");
  const [spansKey, setSpansKey] = useState("");
  const [colorsJson, setColorsJson] = useState("");
  const [sentenceSplit, setSentenceSplit] = useState(false);
  const [markup, setMarkup] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, unknown> | null>(null);
  const [workers, setWorkers] = useState<RenderWorker[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [history, setHistory] = useState<Visualization[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .listWorkers()
      .then(setWorkers)
      .catch(() => setWorkers([]));
    api
      .listRuns()
      .then(setRuns)
      .catch(() => setRuns([]));
    api
      .listVisualizations()
      .then((items) => setHistory(items as Visualization[]))
      .catch(() => setHistory([]));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const worker = workers[0];

  const optionsPayload = useCallback((): Record<string, unknown> => {
    const opts: Record<string, unknown> = {};
    if (ents.trim()) opts.ents = ents.split(",").map((s) => s.trim()).filter(Boolean);
    if (compact) opts.compact = true;
    if (distance) opts.distance = Number(distance);
    if (bg.trim()) opts.bg = bg.trim();
    if (spansKey.trim()) opts.spans_key = spansKey.trim();
    if (colorsJson.trim()) {
      try {
        opts.colors = JSON.parse(colorsJson);
      } catch {
        /* invalid colors JSON is dropped; the remaining options still apply */
      }
    }
    return opts;
  }, [ents, compact, distance, bg, spansKey, colorsJson]);

  const renderReqBody = useCallback(
    () => ({
      worker_id: worker?.id,
      model: worker?.model_path ?? "en_core_web_sm",
      text,
      style,
      options: optionsPayload(),
      sentence_split: sentenceSplit,
    }),
    [worker, text, style, optionsPayload, sentenceSplit],
  );

  const render = useCallback(async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const res = await api.visualize(renderReqBody());
      // displaCy markup embeds raw input text: sanitize before inlining.
      setMarkup(
        DOMPurify.sanitize(res.svg ?? res.html ?? "", SANITIZE_PROFILE),
      );
      setMeta(res.meta);
    } catch (err) {
      setMarkup(null);
      setMeta(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [renderReqBody]);

  const downloadSvg = () => {
    if (!markup) return;
    const blob = new Blob([markup], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "displacy-render.svg";
    a.click();
    URL.revokeObjectURL(url);
  };

  const copySvg = async () => {
    if (!markup) return;
    await navigator.clipboard.writeText(markup);
    setNotice("SVG copied to clipboard.");
  };

  const downloadPng = async () => {
    setBusy(true);
    try {
      const res = await api.visualizePng(renderReqBody());
      const bin = Uint8Array.from(atob(res.png_base64), (ch) => ch.charCodeAt(0));
      const blob = new Blob([bin.buffer as ArrayBuffer], { type: "image/png" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "displacy-render.png";
      a.click();
      URL.revokeObjectURL(url);
      setNotice("PNG downloaded.");
    } catch (err) {
      setNotice(`PNG export failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const saveToRun = async (runId: string) => {
    if (!markup || !runId) return;
    try {
      await api.saveVisualization({
        run_id: runId,
        worker_model: String(meta?.model ?? worker?.model_path ?? "unknown"),
        text,
        style,
        options: optionsPayload(),
        svg: markup,
      });
      setNotice("Visualization attached to run.");
      refresh();
    } catch (err) {
      setNotice(`Save failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  const loadFromHistory = (viz: Visualization) => {
    setMarkup(DOMPurify.sanitize(viz.svg, SANITIZE_PROFILE));
    if (viz.text) setText(viz.text);
    setNotice("Loaded from history — adjust options and press Render to refresh.");
  };

  return (
    <div>
      <div className="page-header">
        <h1>Visualize</h1>
        <p>
          Live displaCy rendering through the coach render worker. Styles:{" "}
          <code>ent</code>, <code>dep</code>, <code>span</code>.
        </p>
      </div>

      {workers.length === 0 && (
        <div className="empty-state">
          <h3>No render worker running</h3>
          <p>
            Start one from a terminal:{" "}
            <code>python -m coach_spacy.render --model en_core_web_sm</code>, or
            spawn it from Settings. Rendering stays unavailable until a worker
            with a loaded pipeline is reachable.
          </p>
        </div>
      )}

      <div className="field-row">
        <select
          className="select"
          value={style}
          onChange={(event) => setStyle(event.target.value as Style)}
          aria-label="displaCy style"
        >
          {STYLES.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            type="checkbox"
            checked={sentenceSplit}
            onChange={(event) => setSentenceSplit(event.target.checked)}
          />
          Sentence by sentence
        </label>
        <input
          className="text-input"
          placeholder="ENT,ORG (comma separated)"
          value={ents}
          onChange={(event) => setEnts(event.target.value)}
          aria-label="ents filter"
        />
        <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            type="checkbox"
            checked={compact}
            onChange={(event) => setCompact(event.target.checked)}
          />
          compact
        </label>
        <input
          className="text-input"
          placeholder="background (#rrggbb)"
          value={bg}
          onChange={(event) => setBg(event.target.value)}
          aria-label="background color"
        />
        {style === "dep" && (
          <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              type="number"
              value={distance}
              onChange={(event) => setDistance(event.target.value)}
              aria-label="distance"
              style={{ width: 80 }}
            />
            distance
          </label>
        )}
        {style === "span" && (
          <input
            className="text-input"
            placeholder="spans_key"
            value={spansKey}
            onChange={(event) => setSpansKey(event.target.value)}
            aria-label="spans key"
          />
        )}
        <input
          className="text-input"
          placeholder='colors: {"PER": "#f0a"}'
          value={colorsJson}
          onChange={(event) => setColorsJson(event.target.value)}
          aria-label="colors JSON"
          style={{ fontFamily: "var(--font-mono)", minWidth: 200 }}
        />
        <button className="button button-primary" onClick={() => void render()} disabled={busy}>
          {busy ? "Rendering…" : "Render"}
        </button>
      </div>

      <textarea
        className="textarea"
        value={text}
        onChange={(event) => setText(event.target.value)}
        aria-label="Text to visualize"
      />

      {error && (
        <div className="empty-state" style={{ marginTop: 16 }}>
          {error}
        </div>
      )}
      {notice && (
        <div className="stat-label" style={{ marginTop: 12 }}>
          {notice}
        </div>
      )}

      {markup && (
        <div className="card" style={{ marginTop: 16 }}>
          <div className="viz-preview" dangerouslySetInnerHTML={{ __html: markup }} />
          {meta && (
            <p className="stat-label" style={{ marginTop: 8 }}>
              {String(meta.model)} · {String(meta.style)} · {String(meta.n_tokens)} tokens ·{" "}
              {String(meta.num_rendered)} rendition(s)
            </p>
          )}
          <div className="field-row" style={{ marginTop: 12 }}>
            <button className="button" onClick={downloadSvg}>
              Download SVG
            </button>
            <button className="button" onClick={() => void copySvg()}>
              Copy SVG
            </button>
            <button className="button" onClick={() => void downloadPng()} disabled={busy}>
              Download PNG
            </button>
            <select
              className="select"
              onChange={(event) => void saveToRun(event.target.value)}
              aria-label="Attach to run"
              value=""
            >
              <option value="">Attach to run…</option>
              {runs.map((run) => (
                <option key={run.id} value={run.id}>
                  {run.name}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      {history.length > 0 && (
        <div className="card" style={{ marginTop: 16 }}>
          <h3>Recent visualizations</h3>
          <table className="table">
            <thead>
              <tr>
                <th>Saved</th>
                <th>Model</th>
                <th>Style</th>
                <th>Run</th>
                <th>Text</th>
              </tr>
            </thead>
            <tbody>
              {history.slice(0, 8).map((viz) => (
                <tr
                  key={viz.id}
                  onClick={() => loadFromHistory(viz)}
                  style={{ cursor: "pointer" }}
                >
                  <td>{new Date(viz.created_at).toLocaleString()}</td>
                  <td>{viz.worker_model}</td>
                  <td>{viz.style}</td>
                  <td>{runs.find((r) => r.id === viz.run_id)?.name ?? "—"}</td>
                  <td style={{ maxWidth: 280, overflow: "hidden", textOverflow: "ellipsis" }}>
                    {viz.text ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
