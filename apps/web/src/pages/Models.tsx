import { useEffect, useState } from "react";
import { Link } from "react-router-dom";

type ModelCandidate = {
  name: string;
  source: "installed" | "workspace" | "registry";
  version?: string;
  summary?: string;
};

/**
 * Model catalog:spaCy pipelines that can finetune.
 * Sources:
 * - workspace scan (parse any /pipeline meta.json in the project)
 * - well-known package-registered models (spaCy hub names)
 * Renders a sortable table of models with their latest metadata and links
 * into the finetuning runs view.
 */
export default function ModelsPage() {
  const [candidates, setCandidates] = useState<ModelCandidate[]>([]);
  const [error] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      const found: ModelCandidate[] = [];
      try {
        const res = await fetch("/api/model-candidates");
        const list = (await res.json()) as string[];
        for (const path of list) {
          found.push({
            name: path.split("/").pop() ?? path,
            source: "workspace",
            summary: path,
          });
        }
      } catch {
        /* no candidates */
      }
      // Well-known spaCy pipelines shown even when not installed so the
      // catalog doubles as a "what can I pull in" reference.
      const registryNames = ["en_core_web_sm", "en_core_web_md", "en_core_web_lg", "de_core_news_sm", "de_core_web_lg"];
      for (const name of registryNames) {
        if (!found.some((c) => c.name === name)) {
          found.push({ name, source: "registry", summary: "spaCy Hub — install via `python -m spacy download`" });
        }
      }
      setCandidates(found);
      setLoading(false);
    })();
  }, []);

  return (
    <div>
      <div className="page-header">
        <h1>Models</h1>
        <p>spaCy pipelines: available for training as hosted-models or packaged pipelines.</p>
      </div>

      {error && <div style={{ color: "var(--error, #c00)" }}>{error}</div>}
      {!loading && candidates.length === 0 && (
        <div className="stat-label">No models found.</div>
      )}

      <table className="table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Source</th>
            <th>Details</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {candidates.map((model) => (
            <tr key={model.name}>
              <td style={{ fontWeight: 600 }}>{model.name}</td>
              <td>
                <span
                  className={
                    model.source === "workspace"
                      ? "badge badge-success"
                      : "badge badge-neutral"
                  }
                >
                  {model.source}
                </span>
              </td>
              <td>{model.summary}</td>
              <td>
                <Link className="button" to={`/training-data/sessions`}>
                  Mit Modell starten
                </Link>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
