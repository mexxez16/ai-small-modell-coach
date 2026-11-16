import { useEffect, useState } from "react";



/** Curation workbench: dataset items with review states. Reads the real
 * datasets API when one exists; falls back to empty guidance otherwise. */
export default function TrainingData() {
  const [datasets, setDatasets] = useState<Array<{ id: string; name: string }>>([]);
  const [active, setActive] = useState<string>("");
  const [items, setItems] = useState<Array<Record<string, unknown>>>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/datasets")
      .then((r) => r.json())
      .then((list) => {
        setDatasets(list);
        if (list.length > 0) setActive(list[0].id);
      })
      .catch((err: Error) => setError("datasets API not reachable: " + err.message));
  }, []);

  useEffect(() => {
    if (!active) return;
    fetch(`/api/datasets/${active}/items`)
      .then((r) => r.json())
      .then((rows) => setItems(rows))
      .catch((err: Error) => setError("items fetch failed: " + err.message));
  }, [active]);

  return (
    <div>
      <div className="page-header">
        <h1>Training Data</h1>
        <p>Datasets curated for this workspace; review states and splits.</p>
      </div>
      {error && <p className="stat-label">{error}</p>}
      {datasets.length === 0 ? (
        <div className="empty-state">
          <h3>No datasets yet</h3>
          <p>
            Create one via <code>POST /api/datasets</code> — the curation workbench lists
            items, review status and split assignment per dataset.
          </p>
        </div>
      ) : (
        <>
          <div className="field-row">
            <select
              className="select"
              value={active}
              onChange={(e) => setActive(e.target.value)}
              aria-label="Dataset"
            >
              {datasets.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name}
                </option>
              ))}
            </select>
          </div>
          <table className="table">
            <thead>
              <tr>
                <th>Text</th>
                <th>Label</th>
                <th>Status</th>
                <th>Split</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, i) => (
                <tr key={i}>
                  <td>{String(item.text ?? "")}</td>
                  <td>{String(item.label ?? "")}</td>
                  <td>{String(item.status ?? "")}</td>
                  <td>{String(item.split ?? "")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}
