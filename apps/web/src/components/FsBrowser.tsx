import { useEffect, useState } from "react";

/**
 * FsBrowser: modal on top of the server-side filesystem listing
 * (`GET /api/fs/list`). Directories open on click, files are selectable —
 * the chosen path is handed back via onPick. Read-only on purpose: the
 * server never writes through this endpoint.
 */
export default function FsBrowser({
  open,
  pick,
  onPick,
  onClose,
}: {
  open: boolean;
  /** what may be selected: a directory or a file */
  pick: "dir" | "any";
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState<string>("");
  const [parent, setParent] = useState<string | null>(null);
  const [entries, setEntries] = useState<Array<{ name: string; path: string; dir: boolean }>>([]);
  const [error, setError] = useState<string | null>(null);

  const load = (p: string) => {
    fetch(`/api/fs/list?path=${encodeURIComponent(p)}`)
      .then((r): Promise<{ path: string; parent: string | null; entries: typeof entries } | { error: string }> => r.json())
      .then((d) => {
        if ("error" in d) {
          setError(d.error);
          return;
        }
        setPath(d.path);
        setParent(d.parent);
        setEntries(d.entries);
        setError(null);
      })
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  };

  useEffect(() => {
    if (open) load(path || ".");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        className="card"
        style={{ width: 560, maxHeight: "70vh", overflow: "auto", display: "flex", flexDirection: "column" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="field-row" style={{ gap: 8, alignItems: "center" }}>
          <b>Pfad wählen</b>
          <code style={{ fontFamily: "var(--font-mono)", fontSize: 12, flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
            {path}
          </code>
          <button className="button" disabled={!parent} onClick={() => parent && load(parent)}>
            ↑ übergeordnet
          </button>
        </div>
        {error && <div className="empty-state">{error}</div>}
        <div style={{ flex: 1, overflow: "auto", marginTop: 8 }}>
          <table className="table">
            <tbody>
              {entries.map((e) => (
                <tr key={e.path}>
                  <td>
                    {e.dir ? (
                      <a href="#" onClick={(ev) => { ev.preventDefault(); load(e.path); }}>
                        {e.name}/
                      </a>
                    ) : (
                      e.name
                    )}
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <button
                      className="button"
                      onClick={() => {
                        onPick(e.path);
                        onClose();
                      }}
                      disabled={pick === "dir" && !e.dir}
                    >
                      Auswählen
                    </button>
                  </td>
                </tr>
              ))}
              {entries.length === 0 && !error && (
                <tr>
                  <td>(leeres Verzeichnis)</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div className="field-row" style={{ justifyContent: "flex-end", marginTop: 8 }}>
          <button className="button" onClick={onClose}>
            Abbrechen
          </button>
          <button
            className="button button-primary"
            onClick={() => {
              onPick(path);
              onClose();
            }}
          >
            Diesen Ordner wählen
          </button>
        </div>
      </div>
    </div>
  );
}
