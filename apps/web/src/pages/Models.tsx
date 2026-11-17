import { useEffect, useMemo, useState } from "react";
import { api, ModelEntry } from "../api/client";
import HelpBubble from "../components/HelpBubble";

type CatalogRow = {
  id: string;
  name: string;
  path: string;
  kind: "shipped" | "local" | "finetuned";
  run_id?: string | null;
  created_at?: string;
};

/** Model catalog: every spaCy pipeline that can be trained/filtered —
 * registry entries (user-added, persisted), fake the workspace scan
 * (auto discovered on disk), and the finetuned outputs of runs. */
export default function ModelsPage() {
  const [entries, setEntries] = useState<ModelEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [kindFilter, setKindFilter] = useState("");

  const [newName, setNewName] = useState("");
  const [newPath, setNewPath] = useState("");
  const [newKind, setNewKind] = useState("local");

  const refresh = async () => {
    try {
      setError(null);
      setEntries(await api.listModels());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };
  useEffect(() => {
    void refresh();
  }, []);

  const addModel = async () => {
    try {
      setError(null);
      await api.createModel(newName.trim(), newPath.trim(), newKind);
      setNewName("");
      setNewPath("");
      setNotice(`Modell "${newName.trim()}" registriert.`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const removeModel = async (id: string) => {
    try {
      await api.deleteModel(id);
      setNotice("Modell entfernt.");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const visible = useMemo(() => {
    let list: CatalogRow[] = entries.map((e) => ({
      id: e.id,
      name: e.name,
      path: e.path,
      kind: e.kind,
      run_id: e.run_id,
      created_at: e.created_at,
    }));
    if (search.trim()) {
      const s = search.trim().toLowerCase();
      list = list.filter(
        (e) => e.name.toLowerCase().includes(s) || e.path.toLowerCase().includes(s),
      );
    }
    if (kindFilter) list = list.filter((e) => e.kind === kindFilter);
    return [...list].sort((a, b) => a.name.localeCompare(b.name));
  }, [entries, search, kindFilter]);

  return (
    <div>
      <div className="page-header">
        <h1>Models</h1>
        <p>
          spaCy-Pipelines, die für Finetuning bereitstehen: registrierte
          Modelle (Pfad oder Hub-Name) plus feingetunte Ergebnisse aus Runs.
        </p>
      </div>

      {error && <div className="empty-state">{error}</div>}
      {notice && <div className="stat-label">{notice}</div>}

      <div className="card" style={{ marginBottom: 16 }}>
        <h3 style={{ marginTop: 0 }}>
          Modell registrieren <HelpBubble helpId="models.register" />
        </h3>
        <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
          <input
            className="text-input"
            placeholder="Anzeigename, z.B. english-small"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            style={{ width: 200 }}
          />
          <input
            className="text-input"
            placeholder="Pfad oder Modname (z.B. en_core_web_sm) "
            value={newPath}
            onChange={(e) => setNewPath(e.target.value)}
            style={{ width: 280, fontFamily: "var(--font-mono)" }}
          />
          <select
            className="select"
            value={newKind}
            onChange={(e) => setNewKind(e.target.value)}
            aria-label="Modelltyp"
          >
            <option value="local">Lokal (Pfad)</option>
            <option value="shipped">Installiert (spaCy Hub)</option>
            <option value="finetuned">Feingetunt</option>
          </select>
          <button
            className="button button-primary"
            disabled={!newName.trim() || !newPath.trim()}
            onClick={() => void addModel()}
          >
            Hinzufügen
          </button>
        </div>
        <p className="stat-label" style={{ marginTop: 8 }}>
          Modellordner öffnen: Der Pfad steht in der Tabelle unten — kopiere ihn
          und öffne ihn im Dateimanager. Aus dem Browser heraus ist kein
          Direktöffnen möglich (Sicherheitsmodell).
        </p>
      </div>

      <div className="field-row" style={{ flexWrap: "wrap", gap: 8, marginBottom: 8 }}>
        <input
          className="text-input"
          placeholder="Name oder Pfad filtern…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 220 }}
        />
        <select
          className="select"
          value={kindFilter}
          onChange={(e) => setKindFilter(e.target.value)}
          aria-label="Typ-Filter"
        >
          <option value="">Typ: alle</option>
          <option value="local">Lokal</option>
          <option value="shipped">Installiert</option>
          <option value="finetuned">Feingetunt</option>
        </select>
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Typ</th>
            <th>Pfad</th>
            <th>Quelle (Run)</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {visible.map((model) => (
            <tr key={model.id}>
              <td style={{ fontWeight: 600 }}>{model.name}</td>
              <td>
                <span className="badge badge-neutral">{model.kind}</span>
              </td>
              <td
                style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}
                title="Kopieren: Klick"
                onClick={() => {
                  void navigator.clipboard?.writeText(model.path);
                  setNotice("Pfad kopiert.");
                }}
              >
                {model.path}
              </td>
              <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                {model.run_id ? `${model.run_id.slice(0, 8)}…` : "—"}
              </td>
              <td>
                <button className="button button-ghost" onClick={() => void removeModel(model.id)}>
                  Entfernen
                </button>
              </td>
            </tr>
          ))}
          {visible.length === 0 && (
            <tr>
              <td colSpan={5}>Keine Modelle registriert.</td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
