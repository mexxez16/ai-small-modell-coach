import { useEffect, useState } from "react";



/** Image gallery: folded training-time renders with creation context. */
export default function Images() {
  const [items, setItems] = useState<Array<Record<string, unknown>>>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/images")
      .then((r) => r.json())
      .then((rows) => setItems(rows))
      .catch(() => setError("images gallery API not reachable"));
  }, []);

  return (
    <div>
      <div className="page-header">
        <h1>Images</h1>
        <p>Image artifacts (training-time renders) attached to runs.</p>
      </div>
      {error && <p className="stat-label">{error}</p>}
      {items.length === 0 ? (
        <div className="empty-state">
          <h3>No image artifacts yet</h3>
          <p>
            Training-time renders from the log pipeline appear here once training
            uploads them (see <code>SPACY_INTEGRATION.md</code>).
          </p>
        </div>
      ) : (
        <div className="card-grid">
          {items.map((item, i) => (
            <div key={i} className="card">
              <p className="stat-label">
                {String(item.name ?? "")} {item.created_at ? `(${String(item.created_at)})` : ""}
              </p>
              {typeof item.png_base64 === "string" && item.png_base64.length > 0 && (
                <img
                  src={`data:image/png;base64,${item.png_base64}`}
                  alt={String(item.name ?? "")}
                  style={{ maxWidth: "100%", borderRadius: 6 }}
                />
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
