import { useCallback, useEffect, useRef, useState } from "react";

import { api, AssistantProposal, Provider } from "../api/client";

type ChatMessage = { role: "user" | "assistant"; content: string };

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

export default function Assistant() {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [providerId, setProviderId] = useState("");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [proposals, setProposals] = useState<AssistantProposal[]>([]);
  const [profile, setProfile] = useState<DatasetProfile[] | null>(null);
  const [spacy, setSpacy] = useState<SpacyInfo | null>(null);
  const [mirrorVersions, setMirrorVersions] = useState<string[]>([]);
  const [docsVersion, setDocsVersion] = useState("v3.8.3");
  const [docQuery, setDocQuery] = useState("");
  const [docHits, setDocHits] = useState<Array<{ file: string; snippet: string }>>([]);
  const threadRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const [p, pr, prof] = await Promise.all([
        api.listProviders(),
        api.assistantProposals(),
        api.assistantDataProfile(),
      ]);
      setProviders(p.providers.filter((x) => x.kind === "openai_compatible"));
      setProposals(pr.proposals);
      setProfile(prof.datasets);
      setProviderId((current) => {
        if (current) return current;
        const first = p.providers.find((x) => x.kind === "openai_compatible");
        return first ? first.id : "";
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [messages]);

  const probeSpacy = async () => {
    setSpacy(null);
    try {
      const info = await api.assistantSpacyStatus();
      setSpacy(info);
    } catch (err) {
      setSpacy({
        python: "python3",
        installed: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const pullDocs = async () => {
    setError(null);
    try {
      await api.assistantDocsPull(docsVersion.trim());
      const status = await api.assistantStatus();
      setMirrorVersions(status.docs_mirror_versions);
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

  const send = async () => {
    const text = input.trim();
    if (!text || busy) return;
    if (!providerId) {
      setError("Zuerst einen OpenAI-kompatiblen Provider in den Einstellungen anlegen.");
      return;
    }
    const next = [...messages, { role: "user" as const, content: text }];
    setMessages(next);
    setInput("");
    setBusy(true);
    try {
      setError(null);
      const res = await api.assistantTurn(providerId, next);
      setMessages([...next, { role: "assistant", content: res.reply || "(leere Antwort)" }]);
      if (res.proposals.length > 0) await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setMessages(next.slice(0, -1));
      setInput(text);
    } finally {
      setBusy(false);
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
        <h1>Assistent</h1>
        <p>
          Ziel-Berater für Trainingsdaten-Läufe. Der Assistent liest das Trainings-Ziel
          samt Konfigurationen, fragt nach, wofür die Daten benötigt werden, und
          schlägt Änderungen als Vorschläge vor — nichts wird ohne deine Bestätigung
          übernommen.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}

      <div className="field-row" style={{ marginBottom: 16 }}>
        <select
          className="select"
          value={providerId}
          onChange={(event) => setProviderId(event.target.value)}
          aria-label="Berater-Provider"
        >
          {providers.length === 0 && <option value="">Kein OpenAI-kompatibler Provider</option>}
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.name}
              {provider.model ? ` · ${provider.model}` : ""}
            </option>
          ))}
        </select>
        <button className="button button-ghost" onClick={() => void probeSpacy()}>
          spaCy prüfen
        </button>
      </div>

      {spacy && (
        <p className="stat-label" style={{ marginBottom: 12 }}>
          spaCy-Umgebung ({spacy.python}):{" "}
          {spacy.installed
            ? `installiert, Version ${spacy.spacy_version ?? "?"}, Modelle: ${
                spacy.installed_models?.join(", ") || "keine"
              }`
            : `nicht installiert${spacy.error ? ` (${spacy.error})` : ""} — Beratung ohne spaCy-Grundlage möglich`}
        </p>
      )}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Beratung</h3>
        <div
          ref={threadRef}
          style={{
            maxHeight: 340,
            overflowY: "auto",
            border: "1px solid var(--border)",
            borderRadius: 8,
            padding: 12,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          {messages.length === 0 && (
            <p className="stat-label" style={{ margin: 0 }}>
              Beispiele: „Wofür eignen sich meine Trainingsdaten?“ oder „Erstelle eine
              Ziel-Konfiguration für eine BFO-Textklassifikation“.
            </p>
          )}
          {messages.map((message, index) => (
            <div
              key={index}
              style={{
                alignSelf: message.role === "user" ? "flex-end" : "flex-start",
                maxWidth: "80%",
                padding: "8px 12px",
                borderRadius: 10,
                background:
                  message.role === "user" ? "var(--surface-alt, rgba(127,127,127,0.15))" : "transparent",
                border: message.role === "user" ? "1px solid var(--border)" : "none",
              }}
            >
              {message.content.split("\n").map((line, lineIndex) => (
                <p key={lineIndex} style={{ margin: lineIndex ? "6px 0 0" : 0 }}>
                  {line}
                </p>
              ))}
            </div>
          ))}
          {busy && <p className="stat-label">Assistent denkt…</p>}
        </div>
        <div className="field-row" style={{ marginTop: 10 }}>
          <input
            className="text-input"
            style={{ flex: 1 }}
            placeholder="Frage oder Anweisung…"
            value={input}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void send();
              }
            }}
            aria-label="Nachricht an den Assistenten"
          />
          <button className="button button-primary" disabled={busy} onClick={() => void send()}>
            Senden
          </button>
        </div>
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>Vorschläge ({proposals.length})</h3>
        {proposals.length === 0 && (
          <p className="stat-label" style={{ margin: 0 }}>
            Keine offenen Vorschläge.
          </p>
        )}
        {proposals.map((proposal) => (
          <div
            key={proposal.id}
            style={{
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: 12,
              marginBottom: 10,
            }}
          >
            <div className="field-row">
              <b>{proposal.summary}</b>
              <span className="stat-label">{new Date(proposal.created_at).toLocaleString()}</span>
              <span className="badge badge-neutral">{proposal.kind}</span>
            </div>
            <pre
              style={{
                fontSize: "var(--text-xs)",
                background: "var(--surface-alt, rgba(127,127,127,0.1))",
                padding: 8,
                borderRadius: 6,
                overflowX: "auto",
                maxHeight: 220,
              }}
            >
              {JSON.stringify(proposal.payload, null, 2)}
            </pre>
            <div className="field-row">
              <button className="button button-primary" onClick={() => void decide(proposal.id, true)}>
                Annehmen
              </button>
              <button className="button" onClick={() => void decide(proposal.id, false)}>
                Verwerfen
              </button>
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
              {dataset.items} Items · ohne Text: {dataset.items_without_text} · Duplikate:{" "}
              {dataset.duplicate_texts}
            </span>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 4 }}>
              {Object.entries(dataset.by_kind).map(([kind, count]) => (
                <span key={kind} className="badge badge-neutral">
                  {kind}: {count}
                </span>
              ))}
            </div>
          </div>
        ))}
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>spaCy-Doku-Mirror</h3>
        <p className="stat-label">
          Ziehe eine Versionsversion der spaCy-Doku als lokalen Spiegel, damit der
          Assistent Änderungen nachsehen kann — auch ohne installiertes spaCy.
          Versionen: {mirrorVersions.length ? mirrorVersions.join(", ") : "keine"}
        </p>
        <div className="field-row">
          <input
            className="text-input"
            style={{ fontFamily: "var(--font-mono)", minWidth: 160 }}
            value={docsVersion}
            onChange={(event) => setDocsVersion(event.target.value)}
            aria-label="spaCy-Doku-Version"
          />
          <button className="button button-primary" onClick={() => void pullDocs()}>
            Docs spiegeln
          </button>
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
