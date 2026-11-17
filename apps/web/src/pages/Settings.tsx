import { useCallback, useEffect, useState } from "react";

import { api, Provider, RenderWorker } from "../api/client";

const PROVIDER_PRESETS: Array<{ label: string; kind: Provider["kind"]; base_url: string; model: string }> = [
  { label: "Mistral API", kind: "openai_compatible", base_url: "https://api.mistral.ai/v1", model: "mistral-small-latest" },
  { label: "Ollama (local)", kind: "openai_compatible", base_url: "http://localhost:11434/v1", model: "llama3.1" },
  { label: "LM Studio (local)", kind: "openai_compatible", base_url: "http://localhost:1234/v1", model: "" },
  { label: "llama.cpp server (local)", kind: "openai_compatible", base_url: "http://localhost:8081/v1", model: "" },
  { label: "Mistral Vibe CLI", kind: "vibe", base_url: "", model: "" },
];

type FormState = {
  id?: string;
  name: string;
  kind: Provider["kind"];
  base_url: string;
  api_key: string;
  model: string;
};

const EMPTY_FORM: FormState = {
  name: "",
  kind: "openai_compatible",
  base_url: "https://api.mistral.ai/v1",
  api_key: "",
  model: "mistral-small-latest",
};

function ProviderForm({
  initial,
  onSave,
  onCancel,
}: {
  initial: FormState;
  onSave: (form: FormState) => Promise<void>;
  onCancel: () => void;
}) {
  const [form, setForm] = useState(initial);
  const set = (patch: Partial<FormState>) => setForm((f) => ({ ...f, ...patch }));

  const applyPreset = (label: string) => {
    const preset = PROVIDER_PRESETS.find((p) => p.label === label);
    if (preset) {
      set({
        name: preset.label,
        kind: preset.kind,
        base_url: preset.base_url,
        model: preset.model,
        api_key: "",
      });
    }
  };

  return (
    <div className="card" style={{ marginBottom: 16 }}>
      <h3 style={{ marginTop: 0 }}>{form.id ? "Edit provider" : "Add provider"}</h3>
      <div className="field-row">
        <select
          className="select"
          onChange={(event) => applyPreset(event.target.value)}
          aria-label="Provider preset"
          defaultValue=""
        >
          <option value="" disabled>
            Preset…
          </option>
          {PROVIDER_PRESETS.map((p) => (
            <option key={p.label} value={p.label}>
              {p.label}
            </option>
          ))}
        </select>
      </div>
      <div className="field-row">
        <input
          className="text-input"
          placeholder="Name"
          value={form.name}
          onChange={(event) => set({ name: event.target.value })}
          aria-label="Provider name"
        />
        <select
          className="select"
          value={form.kind}
          onChange={(event) => set({ kind: event.target.value as Provider["kind"] })}
          aria-label="Provider kind"
        >
          <option value="openai_compatible">OpenAI-compatible API</option>
          <option value="vibe">Mistral Vibe CLI</option>
        </select>
      </div>
      {form.kind === "openai_compatible" && (
        <>
          <div className="field-row">
            <input
              className="text-input"
              style={{ flex: 1, fontFamily: "var(--font-mono)" }}
              placeholder="Base URL (…/v1)"
              value={form.base_url}
              onChange={(event) => set({ base_url: event.target.value })}
              aria-label="Base URL"
            />
            <input
              className="text-input"
              style={{ flex: 1, fontFamily: "var(--font-mono)" }}
              placeholder="Model"
              value={form.model}
              onChange={(event) => set({ model: event.target.value })}
              aria-label="Model"
            />
          </div>
          <div className="field-row">
            <input
              className="text-input"
              type="password"
              style={{ minWidth: 300 }}
              placeholder="API key (empty for local servers)"
              value={form.api_key}
              onChange={(event) => set({ api_key: event.target.value })}
              aria-label="API key"
            />
          </div>
        </>
      )}
      {form.kind === "vibe" && (
        <p className="stat-label">
          Spawns the <code>vibe</code> CLI in a workspace-scoped PTY. The binary
          must be on PATH or set via <code>COACH_VIBE_BIN</code>.
        </p>
      )}
      <div className="field-row">
        <button className="button button-primary" onClick={() => void onSave(form)}>
          Save
        </button>
        <button className="button button-ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

export default function Settings() {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [editing, setEditing] = useState<FormState | null>(null);
  const [testResult, setTestResult] = useState<Record<string, string>>({});
  const [workers, setWorkers] = useState<RenderWorker[]>([]);
  const [modelPath, setModelPath] = useState("en_core_web_sm");
  const [error, setError] = useState<string | null>(null);
  const [vibeModel, setVibeModel] = useState("");
  const [vibePath, setVibePath] = useState<string | null>(null);
  const [vibeStatus, setVibeStatus] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const [p, w] = await Promise.all([api.listProviders(), api.listWorkers()]);
      setProviders(p.providers);
      setWorkers(w);
      const vibe = await api.getVibeSettings();
      setVibeModel(vibe.active_model ?? "");
      setVibePath(vibe.config_path);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveProvider = async (form: FormState) => {
    try {
      await api.upsertProvider({
        id: form.id,
        name: form.name.trim(),
        kind: form.kind,
        base_url: form.base_url.trim() || undefined,
        api_key: form.api_key || undefined,
        model: form.model.trim() || undefined,
      });
      setEditing(null);
      await refresh();
      // Let the chat dock (and other listeners) pick up the new list.
      window.dispatchEvent(new Event("coach.providers-changed"));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const testProvider = async (id: string) => {
    setTestResult((r) => ({ ...r, [id]: "testing…" }));
    try {
      const result = await api.testProvider(id);
      setTestResult((r) => ({ ...r, [id]: `${result.ok ? "OK" : "FAILED"} — ${result.detail}` }));
    } catch (err) {
      setTestResult((r) => ({ ...r, [id]: err instanceof Error ? err.message : String(err) }));
    }
  };

  const spawnWorker = async () => {
    try {
      setError(null);
      await api.spawnWorker(modelPath);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const saveVibeModel = async () => {
    try {
      setError(null);
      const vibe = await api.setVibeModel(vibeModel.trim());
      setVibeModel(vibe.active_model);
      setVibeStatus(`Gespeichert in ${vibe.config_path}`);
    } catch (err) {
      setVibeStatus(null);
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div>
      <div className="page-header">
        <h1>Settings</h1>
        <p>
          Chat providers (persisted in <code>data/providers.json</code>), render
          workers, workspace scope and appearance.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Chat providers</h3>
        <p className="stat-label">
          Any OpenAI-compatible endpoint works: Mistral API, Ollama, LM Studio,
          llama.cpp server, vLLM. Keys are stored server-side and only ever sent
          masked back to the UI.
        </p>
        {editing ? (
          <ProviderForm initial={editing} onSave={saveProvider} onCancel={() => setEditing(null)} />
        ) : (
          <div className="field-row">
            <button
              className="button button-primary"
              onClick={() => setEditing({ ...EMPTY_FORM })}
            >
              Add provider
            </button>
          </div>
        )}

        <table className="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Kind</th>
              <th>Endpoint / Model</th>
              <th>Test</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {providers.map((provider) => (
              <tr key={provider.id}>
                <td>{provider.name}</td>
                <td>
                  <span className="badge badge-neutral">{provider.kind}</span>
                </td>
                <td style={{ fontFamily: "var(--font-mono)", fontSize: "var(--text-xs)" }}>
                  {provider.kind === "vibe"
                    ? "vibe CLI (workspace PTY)"
                    : `${provider.base_url ?? ""} · ${provider.model ?? "?"}`}
                  {provider.api_key ? ` · key ${provider.api_key}` : ""}
                </td>
                <td>
                  {testResult[provider.id] ?? "—"}
                  <button
                    className="button button-ghost"
                    style={{ marginLeft: 8 }}
                    onClick={() => void testProvider(provider.id)}
                  >
                    Test
                  </button>
                </td>
                <td>
                  <button
                    className="button button-ghost"
                    onClick={() =>
                      setEditing({
                        id: provider.id,
                        name: provider.name,
                        kind: provider.kind,
                        base_url: provider.base_url ?? "",
                        api_key: "",
                        model: provider.model ?? "",
                      })
                    }
                  >
                    Edit
                  </button>
                  <button
                    className="button button-ghost"
                    style={{ color: "var(--color-danger)" }}
                    onClick={async () => {
                      await api.deleteProvider(provider.id);
                      await refresh();
                      window.dispatchEvent(new Event("coach.providers-changed"));
                    }}
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {providers.length === 0 && (
              <tr>
                <td colSpan={5}>No providers yet — add one above.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Mistral Vibe: Standard-Modell</h3>
        <p className="stat-label">
          Das Mistral Vibe CLI wählt sein Modell über den Eintrag{" "}
          <code>active_model</code> in seiner eigenen Konfigurationsdatei
          {vibePath ? <span> (<code>{vibePath}</code>)</span> : null}. Hier kannst
          du dieses Modell lesen und überschreiben — es gilt dann für Chat und
          Headless-Aufrufe des Vibe-Agenten (z.&#8202;B. <code>vibe -p …</code>).
          Die CLI selbst besitzt keinen <code>--model</code>-Schalter, daher
          schreibt der Coach direkt in die Konfiguration.
        </p>
        <div className="field-row">
          <input
            className="text-input"
            style={{ flex: 1, fontFamily: "var(--font-mono)", minWidth: 300 }}
            placeholder="z. B. mistral-small-latest"
            value={vibeModel}
            onChange={(event) => {
              setVibeModel(event.target.value);
              setVibeStatus(null);
            }}
            aria-label="Mistral Vibe Modell"
          />
          <button
            className="button button-primary"
            disabled={!vibeModel.trim()}
            onClick={() => void saveVibeModel()}
          >
            Übernehmen
          </button>
          <button className="button button-ghost" onClick={() => void refresh()}>
            Neu laden
          </button>
        </div>
        {vibeStatus && <p className="stat-label">{vibeStatus}</p>}
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Visualisierung: spaCy Render-Dienste</h3>
        <p className="stat-label">
          Ein Render-Dienst ist ein lokaler Python-Prozess, der ein spaCy-Modell lädt und
          Dokumente als displaCy-Grafik (Entitäten, Dependency-Bäume, Spans) zeichnet.
          Er wird von der Seite „Visualize“ und dem Export „.spacy“ benutzt. Man startet
          ihn mit einem Modellpfad (installierter spaCy-Pipeline, z.B. einem Paket oder
          einem trainierten Pipeline-Ordner) — ein Klick auf „Dienst starten“.
          Mehrere Modelle können parallel laufen; beendet man den Server,
          enden auch die Dienste.
        </p>
        <div className="field-row">
          <input
            className="text-input"
            style={{ minWidth: 280, fontFamily: "var(--font-mono)" }}
            value={modelPath}
            onChange={(event) => setModelPath(event.target.value)}
            placeholder="en_core_web_sm oder /pfad/zum/pipeline-ordner"
            aria-label="spaCy-Modellpfad"
          />
          <button className="button button-primary" onClick={() => void spawnWorker()}>
            Dienst starten
          </button>
        </div>
        <div className="field-row">
          {[
            "en_core_web_sm",
            "en_core_web_md",
            "en_core_web_lg",
            "de_core_news_md",
            "de_core_news_lg",
          ].map((preset) => (
            <button
              key={preset}
              className="button button-ghost"
              onClick={() => setModelPath(preset)}
            >
              {preset}
            </button>
          ))}
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>Model</th>
              <th>Status</th>
              <th>PID</th>
              <th>Started</th>
            </tr>
          </thead>
          <tbody>
            {workers.map((worker) => (
              <tr key={worker.id}>
                <td style={{ fontFamily: "var(--font-mono)" }}>{worker.model_path}</td>
                <td>
                  <span className="badge badge-neutral">{worker.status}</span>
                </td>
                <td>{worker.pid ?? "—"}</td>
                <td>{new Date(worker.started_at).toLocaleString()}</td>
              </tr>
            ))}
            {workers.length === 0 && (
              <tr>
                <td colSpan={4}>No workers running.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>Workspace &amp; appearance</h3>
        <p className="stat-label">
          The embedded Vibe agent and terminal are always scoped to the
          workspace directory coach-api was started with (override with
          COACH_WORKSPACE); no host-wide access. Theme: use the toggle in the
          top bar; docks, theme and provider choice persist locally.
        </p>
      </div>
    </div>
  );
}
