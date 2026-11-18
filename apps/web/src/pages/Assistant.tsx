import { useCallback, useEffect, useState } from "react";

import { api, AssistantProfile, AssistantProposal } from "../api/client";

type DatasetProfile = {
  dataset_id: string;
  name: string;
  items: number;
  by_kind: Record<string, number>;
  by_label: Record<string, number>;
  items_without_text: number;
  duplicate_texts: number;
};

type SpacyInfo = {
  python: string;
  installed: boolean;
  spacy_version?: string | null;
  installed_models?: string[];
  error?: string | null;
};

const KNOWN_TOOLS = [
  { id: "docs_search", label: "Doku-Suche (docs_search)" },
  { id: "get_data_profile", label: "Datenprofil (get_data_profile)" },
  { id: "get_app_status", label: "App-Status (get_app_status)" },
  { id: "get_run_status", label: "Run-Status (get_run_status)" },
  { id: "propose_step_config", label: "Step-Config vorschlagen" },
  { id: "propose_goal", label: "Trainings-Ziel vorschlagen" },
  { id: "handoff", label: "Weiterleiten (handoff)" },
];

export default function Assistant() {
  const [error, setError] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<AssistantProfile[]>([]);
  const [drafts, setDrafts] = useState<Record<string, AssistantProfile>>({});
  const [savedNote, setSavedNote] = useState<string | null>(null);
  const [defaultProvider, setDefaultProvider] = useState<string | null>(null);
  const [defaultOptions, setDefaultOptions] = useState<Array<{ id: string; name: string; model: string | null }>>([]);
  const [proposals, setProposals] = useState<AssistantProposal[]>([]);
  const [profile, setProfile] = useState<DatasetProfile[] | null>(null);
  const [spacy, setSpacy] = useState<SpacyInfo | null>(null);
  const [mirrorVersions, setMirrorVersions] = useState<string[]>([]);
  const [docsVersion, setDocsVersion] = useState("v3.8.3");
  const [docQuery, setDocQuery] = useState("");
  const [docHits, setDocHits] = useState<Array<{ file: string; snippet: string }>>([]);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const [p, pr, prof, def] = await Promise.all([
        api.assistantProfiles(),
        api.assistantProposals(),
        api.assistantDataProfile(),
        api.getAssistantDefault(),
      ]);
      setProfiles(p.profiles);
      setDrafts(Object.fromEntries(p.profiles.map((x) => [x.id, { ...x, allowed_tools: [...x.allowed_tools], starter_prompts: [...x.starter_prompts] }])));
      setProposals(pr.proposals);
      setProfile(prof.datasets);
      setDefaultProvider(def.default_assistant);
      setDefaultOptions(def.providers);
      const st = await api.assistantStatus();
      setMirrorVersions(st.docs_mirror_versions);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const draftFor = (id: string) => drafts[id] ?? profiles.find((p) => p.id === id);

  const patchDraft = (id: string, patch: Partial<AssistantProfile>) =>
    setDrafts((all) => ({ ...all, [id]: { ...(all[id] ?? profiles.find((p) => p.id === id)!), ...patch } }));

  const saveProfile = async (id: string) => {
    const draft = drafts[id];
    if (!draft) return;
    try {
      setError(null);
      setSavedNote(null);
      await api.putAssistantProfile(id, {
        ...draft,
        starter_prompts: draft.starter_prompts.filter((line) => line.trim()),
      });
      setSavedNote(`Profil ${draft.name} gespeichert.`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const saveDefault = async (id: string | null) => {
    try {
      setError(null);
      await api.setAssistantDefault(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const probeSpacy = async () => {
    try {
      setSpacy(await api.assistantSpacyStatus());
    } catch (err) {
      setSpacy({ python: "python3", installed: false, error: err instanceof Error ? err.message : String(err) });
    }
  };

  const pullDocs = async () => {
    setError(null);
    try {
      await api.assistantDocsPull(docsVersion.trim());
      const st = await api.assistantStatus();
      setMirrorVersions(st.docs_mirror_versions);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const searchDocs = async () => {
    setError(null);
    try {
      const res = await api.assistantDocsSearch(docQuery);
      setDocHits(res.matches);
    } catch (err) {
      setDocHits([]);
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const decide = async (id: string, accept: boolean) => {
    try {
      if (accept) await api.assistantAcceptProposal(id);
      else await api.assistantRejectProposal(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div>
      <div className="page-header">
        <h1>Assistenten</h1>
        <p>
          Konfiguration der drei Assistent-Profile, die im Chat-Dock geführt werden:
          „Small Model Coach“ (Organisator mit Weiterleitung), „Data“ (Trainingsdaten) und
          „Finetuning“ (Sessions/Runs/Modelle). Nie ändert ein Assistent etwas selbst —
          Vorschläge landen in der Warteschlange unten.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Modell für die Assistenten</h3>
        {defaultOptions.length === 0 ? (
          <p className="stat-label" style={{ margin: 0 }}>
            Kein OpenAI-kompatibler Provider vorhanden — zuerst in Settings → Chat providers anlegen.
          </p>
        ) : (
          <div className="field-row">
            <select
              className="select"
              value={defaultProvider ?? ""}
              onChange={(event) => void saveDefault(event.target.value || null)}
              aria-label="Standard-Provider für Assistenten"
            >
              <option value="">— automatisch (erster Provider) —</option>
              {defaultOptions.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.name}
                  {provider.model ? ` · ${provider.model}` : ""}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      {profiles.map((profile) => {
        const draft = draftFor(profile.id)!;
        return (
          <div key={profile.id} className="card" style={{ marginBottom: 16 }}>
            <h3 style={{ marginTop: 0 }}>{profile.name}</h3>
            <p className="stat-label">{profile.description}</p>
            <div className="field-row">
              <label className="stat-label" style={{ minWidth: 120 }}>System-Prompt</label>
              <textarea
                className="text-input"
                style={{ flex: 1, minHeight: 90, fontFamily: "var(--font-mono)" }}
                value={draft.system_prompt}
                onChange={(event) => patchDraft(profile.id, { system_prompt: event.target.value })}
                aria-label={`System-Prompt ${profile.name}`}
              />
            </div>
            <div className="field-row">
              <label className="stat-label" style={{ minWidth: 120 }}>Werkzeuge</label>
              <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
                {KNOWN_TOOLS.map((tool) => (
                  <label key={tool.id} style={{ display: "flex", gap: 4, alignItems: "center" }}>
                    <input
                      type="checkbox"
                      checked={draft.allowed_tools.includes(tool.id)}
                      onChange={(event) =>
                        patchDraft(profile.id, {
                          allowed_tools: event.target.checked
                            ? [...draft.allowed_tools, tool.id]
                            : draft.allowed_tools.filter((t) => t !== tool.id),
                        })
                      }
                    />
                    <span>{tool.label}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="field-row">
              <label className="stat-label" style={{ minWidth: 120 }}>Start-Prompts</label>
              <textarea
                className="text-input"
                style={{ flex: 1, minHeight: 70, fontFamily: "var(--font-mono)" }}
                value={draft.starter_prompts.join("\n")}
                onChange={(event) => patchDraft(profile.id, { starter_prompts: event.target.value.split("\n") })}
                aria-label={`Start-Prompts ${profile.name}`}
                placeholder="Eine Zeile pro Chip"
              />
            </div>
            <div className="field-row">
              <button className="button button-primary" onClick={() => void saveProfile(profile.id)}>
                Speichern
              </button>
              <button className="button button-ghost" onClick={() => patchDraft(profile.id, { ...profile })}>
                Zurücksetzen
              </button>
            </div>
          </div>
        );
      })}
      {savedNote && <p className="stat-label">{savedNote}</p>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Vorschläge ({proposals.length})</h3>
        {proposals.length === 0 && (
          <p className="stat-label" style={{ margin: 0 }}>Keine offenen Vorschläge.</p>
        )}
        {proposals.map((proposal) => (
          <div key={proposal.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 10 }}>
            <div className="field-row">
              <b>{proposal.summary}</b>
              <span className="stat-label">{new Date(proposal.created_at).toLocaleString()}</span>
              <span className="badge badge-neutral">{proposal.kind}</span>
            </div>
            <pre style={{ fontSize: "var(--text-xs)", background: "var(--surface-alt, rgba(127,127,127,0.1))", padding: 8, borderRadius: 6, overflowX: "auto", maxHeight: 220 }}>
              {JSON.stringify(proposal.payload, null, 2)}
            </pre>
            <div className="field-row">
              <button className="button button-primary" onClick={() => void decide(proposal.id, true)}>Annehmen</button>
              <button className="button" onClick={() => void decide(proposal.id, false)}>Verwerfen</button>
            </div>
          </div>
        ))}
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Datenprofil</h3>
        {profile === null && <p className="stat-label">Lade…</p>}
        {profile?.length === 0 && <p className="stat-label" style={{ margin: 0 }}>Noch keine Datasets.</p>}
        {profile?.map((dataset) => (
          <div key={dataset.dataset_id} style={{ marginBottom: 12 }}>
            <b>{dataset.name}</b>{" "}
            <span className="stat-label">
              {dataset.items} Items · ohne Text: {dataset.items_without_text} · Duplikate: {dataset.duplicate_texts}
            </span>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 4 }}>
              {Object.entries(dataset.by_kind).map(([kind, count]) => (
                <span key={kind} className="badge badge-neutral">{kind}: {count}</span>
              ))}
            </div>
          </div>
        ))}
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>spaCy-Umgebung &amp; Doku-Spiegel</h3>
        <div className="field-row">
          <button className="button button-ghost" onClick={() => void probeSpacy()}>spaCy prüfen</button>
        </div>
        {spacy && (
          <p className="stat-label" style={{ marginBottom: 12 }}>
            spaCy-Umgebung ({spacy.python}):{" "}
            {spacy.installed
              ? `installiert, Version ${spacy.spacy_version ?? "?"}, Modelle: ${spacy.installed_models?.join(", ") || "keine"}`
              : `nicht installiert${spacy.error ? ` (${spacy.error})` : ""} — Beratung ohne spaCy-Grundlage möglich`}
          </p>
        )}
        <p className="stat-label">
          Ziehe eine Doku-Version als lokalen Spiegel, damit die Assistenten Details nachschlagen
          können — auch ohne installiertes spaCy. Versionen: {mirrorVersions.length ? mirrorVersions.join(", ") : "keine"}
        </p>
        <div className="field-row">
          <input
            className="text-input"
            style={{ fontFamily: "var(--font-mono)", minWidth: 160 }}
            value={docsVersion}
            onChange={(event) => setDocsVersion(event.target.value)}
            aria-label="spaCy-Doku-Version"
          />
          <button className="button button-primary" onClick={() => void pullDocs()}>Docs spiegeln</button>
        </div>
        <div className="field-row" style={{ marginTop: 8 }}>
          <input
            className="text-input"
            style={{ flex: 1 }}
            placeholder="Doku durchsuchen (z. B. textcat_multilabel)"
            value={docQuery}
            onChange={(event) => setDocQuery(event.target.value)}
            aria-label="Docs-Suche"
          />
          <button className="button button-ghost" disabled={!mirrorVersions.length || !docQuery.trim()} onClick={() => void searchDocs()}>
            Suchen
          </button>
        </div>
        {docHits.map((hit, index) => (
          <p key={index} className="stat-label" style={{ margin: "4px 0" }}>
            <code>{hit.file}</code>: {hit.snippet}
          </p>
        ))}
      </div>
    </div>
  );
}
