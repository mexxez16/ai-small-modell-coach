import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api, Run, Scope } from "../api/client";

type QueryResult = {
  parsed: string;
  note: string;
  matched_runs: Run[];
};

type SortKey = "name" | "status" | "model" | "created_at";

/** Runs overview: sortable/filterable table of every training run, plus the
 * advanced query box and saved scopes. */
export default function Runs() {
  const [runs, setRuns] = useState<Run[]>([]);
  const [sessions, setSessions] = useState<Array<{ id: string; name: string }>>([]);
  const [models, setModels] = useState<Array<{ id: string; name: string; path: string }>>([]);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [sessionFilter, setSessionFilter] = useState("");
  const [modelFilter, setModelFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("created_at");
  const [sortAsc, setSortAsc] = useState(false);

  const [query, setQuery] = useState("");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scopes, setScopes] = useState<Scope[]>([]);
  const [scopeName, setScopeName] = useState("");
  const navigate = useNavigate();

  const loadScopes = useCallback(() => {
    api.listScopes().then(setScopes).catch(() => undefined);
  }, []);
  useEffect(loadScopes, [loadScopes]);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const [runList, sessionList, modelList] = await Promise.all([
        api.listRuns(),
        api.listSessions(),
        api.listModels(),
      ]);
      setRuns(runList);
      setSessions(sessionList);
      setModels(modelList.map((m) => ({ id: m.id, name: m.name, path: m.path })));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setRuns([]);
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runQueryWith = useCallback(async (q: string) => {
    try {
      setError(null);
      setResult(await api.queryRuns(q));
    } catch (err) {
      setResult(null);
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);
  const runQuery = () => (query.trim() ? runQueryWith(query) : undefined);

  const saveScope = async () => {
    if (!query.trim() || !scopeName.trim()) return;
    await api.saveScope(scopeName.trim(), query.trim());
    setScopeName("");
    loadScopes();
  };
  const applyScope = (scope: Scope) => {
    setQuery(scope.query);
    void runQueryWith(scope.query);
  };

  const visible = useMemo(() => {
    let list = result ? result.matched_runs : runs;
    if (search.trim()) {
      const s = search.trim().toLowerCase();
      list = list.filter((r) => r.name.toLowerCase().includes(s));
    }
    if (statusFilter) list = list.filter((r) => r.status === statusFilter);
    if (sessionFilter)
      list = list.filter((r) =>
        sessionFilter === "__none" ? !r.session_id : r.session_id === sessionFilter,
      );
    if (modelFilter)
      list = list.filter((r) =>
        modelFilter === "__none" ? !r.model : r.model === modelFilter,
      );
    const dir = sortAsc ? 1 : -1;
    return [...list].sort((a, b) => {
      const v =
        sortKey === "created_at"
          ? new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
          : String(a[sortKey] ?? "").localeCompare(String(b[sortKey] ?? ""));
      return v * dir;
    });
  }, [runs, result, search, statusFilter, sessionFilter, modelFilter, sortKey, sortAsc]);

  const sessionName = (id?: string | null) =>
    id ? (sessions.find((s) => s.id === id)?.name ?? "—") : "—";

  const sortHeader = (key: SortKey, label: string) => (
    <th
      style={{ cursor: "pointer", userSelect: "none" }}
      onClick={() => {
        if (sortKey === key) setSortAsc((v) => !v);
        else {
          setSortKey(key);
          setSortAsc(true);
        }
      }}
    >
      {label}
      {sortKey === key ? (sortAsc ? " ▲" : " ▼") : ""}
    </th>
  );

  return (
    <div>
      <div className="page-header">
        <h1>Runs</h1>
        <p>
          Alle Finetuning-Läufe — ein Run trainiert ein SpaCy-Modell mit einem Datensatz.
          Filter über Session/Modell/Datensatz; die Query-Box bleibt als erweiterte
          Suche verfügbar.
        </p>
      </div>

      <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
        <input
          className="text-input"
          placeholder="Name suchen…"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          style={{ width: 200 }}
        />
        <select
          className="select"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
          aria-label="Status-Filter"
        >
          <option value="">Status: alle</option>
          <option value="running">running</option>
          <option value="completed">completed</option>
          <option value="failed">failed</option>
        </select>
        <select
          className="select"
          value={sessionFilter}
          onChange={(e) => setSessionFilter(e.target.value)}
          aria-label="Session-Filter"
        >
          <option value="">Session: alle</option>
          <option value="__none">keine Session</option>
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <select
          className="select"
          value={modelFilter}
          onChange={(e) => setModelFilter(e.target.value)}
          aria-label="Modell-Filter"
        >
          <option value="">Modell: alle</option>
          <option value="__none">kein Modell</option>
          {models.map((m) => (
            <option key={m.id} value={m.name}>
              {m.name}
            </option>
          ))}
        </select>
        {result && (
          <button className="button button-ghost" onClick={() => setResult(null)}>
            Query-Filter zurücksetzen
          </button>
        )}
      </div>

      <div className="field-row" style={{ flexWrap: "wrap", gap: 8 }}>
        <input
          className="text-input"
          style={{ minWidth: 320, fontFamily: "var(--font-mono)" }}
          placeholder='erweitert: run.name == "x" and metrics.cats_score > 0.9'
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void runQuery()}
        />
        <button className="button" onClick={() => void runQuery()}>
          Query anwenden
        </button>
        <input
          className="text-input"
          placeholder="Scope-Name"
          value={scopeName}
          onChange={(event) => setScopeName(event.target.value)}
          aria-label="Scope name"
          style={{ maxWidth: 150 }}
        />
        <button
          className="button"
          onClick={() => void saveScope()}
          disabled={!query.trim() || !scopeName.trim()}
        >
          Scope speichern
        </button>
        {scopes.map((scope) => (
          <span key={scope.id} style={{ display: "flex", gap: 4, alignItems: "center" }}>
            <button
              className="button button-ghost"
              onClick={() => applyScope(scope)}
              title={scope.query}
            >
              {scope.name}
            </button>
            <button
              className="button button-ghost"
              aria-label={`delete scope ${scope.name}`}
              onClick={() => void api.deleteScope(scope.id).then(loadScopes)}
            >
              x
            </button>
          </span>
        ))}
      </div>

      {result && (
        <div className="card">
          <p className="stat-label">Query gefiltert</p>
          <p style={{ fontFamily: "var(--font-mono)" }}>{result.parsed || "(match all)"}</p>
          <p className="stat-label">Treffer: {result.matched_runs.length}</p>
        </div>
      )}
      {error && <div className="empty-state">{error}</div>}

      <div className="card">
        <table className="table">
          <thead>
            <tr>
              {sortHeader("name", "Run")}
              {sortHeader("status", "Status")}
              {sortHeader("model", "Modell")}
              <th>Session</th>
              <th>Dataset</th>
              {sortHeader("created_at", "Erstellt")}
            </tr>
          </thead>
          <tbody>
            {visible.map((run) => (
              <tr
                key={run.id}
                onClick={() => navigate(`/runs/${run.id}`)}
                style={{ cursor: "pointer" }}
              >
                <td>{run.name}</td>
                <td>
                  <span
                    className={`badge badge-${run.status === "completed" ? "success" : run.status === "failed" ? "danger" : "neutral"}`}
                  >
                    {run.status}
                  </span>
                </td>
                <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                  {run.model ?? "—"}
                </td>
                <td>{sessionName(run.session_id)}</td>
                <td style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
                  {run.dataset_id ? `${run.dataset_id.slice(0, 8)}…` : "—"}
                </td>
                <td>{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {visible.length === 0 && (
              <tr>
                <td colSpan={6}>Keine Runs — Filter ändern oder ersten Run erstellen.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
