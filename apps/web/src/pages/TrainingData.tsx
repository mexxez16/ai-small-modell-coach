type Stage = "raw" | "curated" | "approved" | "rejected";

type Item = {
  id: string;
  text: string;
  label: string;
  source: string;
  status: Stage;
  split: string;
};

// Scaffold sample rows; the curation workbench reads /api/datasets/{id}/items
// once the storage backend lands.
const SAMPLE_ITEMS: Item[] = [
  {
    id: "E27",
    text: "The refund was processed within two days.",
    label: "billing-refund",
    source: "BFO_Entity_Classification.json#E27",
    status: "approved",
    split: "train",
  },
  {
    id: "E28",
    text: "Unknown error while syncing the ledger.",
    label: "error-report",
    source: "BFO_Entity_Classification.json#E28",
    status: "raw",
    split: "unassigned",
  },
  {
    id: "E29",
    text: "Where is my order confirmation?",
    label: "order-status",
    source: "BFO_Entity_Classification.json#E29",
    status: "curated",
    split: "dev",
  },
];

export default function TrainingData() {
  return (
    <div>
      <div className="page-header">
        <h1>Training Data</h1>
        <p>
          Curation workbench: review states (raw, curated, approved, rejected),
          audit trail, dataset versioning, and the SSOT to JSONL to DocBin
          conversion pipeline.
        </p>
      </div>

      <div className="card">
        <table className="table">
          <thead>
            <tr>
              <th>Source</th>
              <th>Text</th>
              <th>Label</th>
              <th>Status</th>
              <th>Split</th>
              <th>Visualize</th>
            </tr>
          </thead>
          <tbody>
            {SAMPLE_ITEMS.map((item) => (
              <tr key={item.id}>
                <td style={{ fontFamily: "var(--font-mono)", fontSize: "var(--text-xs)" }}>
                  {item.source}
                </td>
                <td>{item.text}</td>
                <td>
                  <span className="badge badge-neutral">{item.label}</span>
                </td>
                <td>
                  <span
                    className={`badge badge-${
                      item.status === "approved"
                        ? "success"
                        : item.status === "rejected"
                          ? "danger"
                          : item.status === "curated"
                            ? "neutral"
                            : "warning"
                    }`}
                  >
                    {item.status}
                  </span>
                </td>
                <td>{item.split}</td>
                <td>
                  <a className="button button-ghost" href={`/visualize?text=${encodeURIComponent(item.text)}`}>
                    Visualize
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="empty-state">
        <h3>Conversion pipeline</h3>
        <p>
          SSOT JSON to seed JSONL preview, stratified train/dev/eval DocBin
          export (per-head, seeded) lands with the dataset feature. Labels are
          normalized to kebab-case into a single canonical label space.
        </p>
      </div>
    </div>
  );
}
