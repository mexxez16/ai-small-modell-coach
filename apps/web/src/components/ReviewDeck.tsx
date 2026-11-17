import { useCallback, useEffect, useState } from "react";

type ReviewItem = {
  id: string;
  text?: string | null;
  subject?: string | null;
  predicate?: string | null;
  object?: string | null;
  label: string;
  status: string;
  source: string;
};

type Props = {
  items: ReviewItem[];
  onClose: () => void;
  /** Called for each decision; re-fetches are the caller's job. */
  onDecide: (itemId: string, action: "approved" | "rejected") => Promise<void>;
  onEditLabel: (itemId: string, label: string) => Promise<void>;
};

/**
 * Card-mode review: one item per screen, decisions by keyboard.
 *   y = approve   r = reject   e = edit label   k/arrow = skip
 * Items already decided (approved/rejected) are skipped automatically.
 */
export default function ReviewDeck({ items, onClose, onDecide, onEditLabel }: Props) {
  const [index, setIndex] = useState(0);
  const [editing, setEditing] = useState(false);
  const [labelDraft, setLabelDraft] = useState("");
  const [done, setDone] = useState(0);

  const reviewable = items.filter((i) => i.status !== "rejected");
  const item = reviewable[index];

  const advance = useCallback(() => setIndex((n) => n + 1), []);

  useEffect(() => {
    if (!item) onClose();
  }, [item, onClose]);

  const decide = useCallback(
    async (action: "approved" | "rejected") => {
      if (!item) return;
      await onDecide(item.id, action);
      setDone((n) => n + 1);
      advance();
    },
    [item, onDecide, advance],
  );

  const commitLabel = useCallback(
    async (label: string) => {
      if (!item || !label.trim()) return;
      await onEditLabel(item.id, label.trim());
      setEditing(false);
      setLabelDraft("");
    },
    [item, onEditLabel],
  );

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement) return; // typing the label
      switch (event.key.toLowerCase()) {
        case "y":
          void decide("approved");
          break;
        case "r":
          void decide("rejected");
          break;
        case "k":
        case "arrowright":
          advance();
          break;
        case "e":
          setEditing(true);
          setLabelDraft(item?.label ?? "");
          break;
        case "escape":
          onClose();
          break;
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [decide, advance, onClose, item]);

  if (!item) {
    return (
      <div className="card">
        <h3>Review abgeschlossen</h3>
        <p className="stat-label">{done} Items entschieden.</p>
        <button className="button button-primary" onClick={onClose}>
          Zurück zur Tabelle
        </button>
      </div>
    );
  }

  const text = item.text ?? `${item.subject ?? "?"} — ${item.predicate ?? "?"} → ${item.object ?? "?"}`;

  return (
    <div
      className="card"
      role="region"
      aria-label="Review-Karte"
      style={{ maxWidth: 640, margin: "12px auto", padding: "var(--space-5)" }}
    >
      <div className="field-row" style={{ justifyContent: "space-between" }}>
        <span className="stat-label">
          Karte {index + 1}/{reviewable.length} · {done} entschieden
        </span>
        <button className="button button-ghost" onClick={onClose} title="Esc">
          Beenden
        </button>
      </div>

      <p
        style={{ fontSize: 16, lineHeight: 1.6, margin: "var(--space-4) 0" }}
        aria-live="polite"
      >
        {text}
      </p>

      <div className="field-row" style={{ gap: 8, alignItems: "center" }}>
        {editing ? (
          <>
            <input
              className="input"
              autoFocus
              value={labelDraft}
              onChange={(e) => setLabelDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void commitLabel(labelDraft);
              }}
              placeholder="neues Label (kebab-case)"
              style={{ width: 220 }}
            />
            <button
              className="button button-primary"
              onClick={() => void commitLabel(labelDraft)}
            >
              Speichern (Enter)
            </button>
          </>
        ) : (
          <span
            className="badge badge-neutral"
            style={{ fontFamily: "var(--font-mono)" }}
          >
            {item.label}
          </span>
        )}
        <span className="stat-label">{item.source}</span>
      </div>

      <div className="field-row" style={{ gap: 8, marginTop: "var(--space-4)" }}>
        <button className="button button-primary" onClick={() => void decide("approved")}>
          Genehmigen <span className="stat-label">(y)</span>
        </button>
        <button className="button" onClick={() => void decide("rejected")}>
          Verwerfen <span className="stat-label">(r)</span>
        </button>
        <button className="button" onClick={() => setEditing(true)}>
          Label ändern <span className="stat-label">(e)</span>
        </button>
        <button className="button button-ghost" onClick={advance}>
          Überspringen <span className="stat-label">(k)</span>
        </button>
      </div>
    </div>
  );
}
