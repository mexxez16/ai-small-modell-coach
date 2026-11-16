import DOMPurify from "dompurify";
import { useCallback, useEffect, useState } from "react";

import { api, RenderWorker } from "../api/client";

const STYLES = ["ent", "dep", "span"] as const;
type Style = (typeof STYLES)[number];

export default function Visualize() {
  const [text, setText] = useState(
    "Apple is looking at buying U.K. startup for $1 billion.",
  );
  const [style, setStyle] = useState<Style>("ent");
  const [options, setOptions] = useState("{}");
  const [sentenceSplit, setSentenceSplit] = useState(false);
  const [markup, setMarkup] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, unknown> | null>(null);
  const [workers, setWorkers] = useState<RenderWorker[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listWorkers()
      .then(setWorkers)
      .catch(() => setWorkers([]));
  }, []);

  const render = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const parsedOptions = JSON.parse(options) as Record<string, unknown>;
      const worker = workers[0];
      const res = await api.visualize({
        worker_id: worker?.id,
        model: worker?.model_path ?? "en_core_web_sm",
        text,
        style,
        options: parsedOptions,
        sentence_split: sentenceSplit,
      });
      // displaCy markup embeds raw input text: sanitize before inlining.
      setMarkup(DOMPurify.sanitize(res.svg ?? res.html ?? "", { USE_PROFILES: { svg: true, svgFilters: true, html: true } }));
      setMeta(res.meta);
    } catch (err) {
      setMarkup(null);
      setMeta(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [options, sentenceSplit, style, text, workers]);

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
          style={{ fontFamily: "var(--font-mono)", minWidth: 260 }}
          value={options}
          onChange={(event) => setOptions(event.target.value)}
          placeholder='{"distance": 120}'
          aria-label="displaCy options (JSON)"
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

      {error && <div className="empty-state" style={{ marginTop: 16 }}>{error}</div>}

      {markup && (
        <div className="card" style={{ marginTop: 16 }}>
          <div className="viz-preview" dangerouslySetInnerHTML={{ __html: markup }} />
          {meta && (
            <p className="stat-label" style={{ marginTop: 8 }}>
              {String(meta.model)} · {String(meta.style)} · {String(meta.n_tokens)} tokens ·{" "}
              {String(meta.num_rendered)} rendition(s)
            </p>
          )}
        </div>
      )}
    </div>
  );
}
