/**
 * HelpBubble: reusable "?" circle with a hover/focus popup explaining a
 * pipeline step. Texts and examples come from helpTexts below; every entry
 * is shown verbatim, so they double as living documentation.
 */

export type HelpEntry = {
  title: string;
  body: string;
  example?: string;
};

export const helpTexts: Record<string, HelpEntry> = {
  "train-data.sources": {
    title: "Was ist eine Quelle?",
    body: "Eine Quelle ist ein hochgeladenes Dokument (Text, Markdown, JSON, SSOT-Export). Sie wird mit sha256 gespeichert und ist die Basis für das Chunking — nichts landet ohne nachvollziehbare Herkunft im Datensatz.",
    example: 'POST /api/sources  { name: "BFO_SSOT.txt", content: "..." }',
  },
  "train-data.chunking": {
    title: "Chunking-Regeln",
    body: "Absätze eignen sich für Fließtext, Sätze für präzise Kleinkorpora, Fenster für strukturlange Dokumente. Jeder Chunk wird ein DataItem mit Herkunft Quelle#Nr und Status raw.",
    example: "Absatz: teilt bei Leerzeilen\nSatz: teilt bei . ! ?\nFenster: feste Blöcke mit Überlappung",
  },
  "train-data.strategy": {
    title: "Chunking-Strategie",
    body: "„Absatz“ bewahrt thematische Blöcke, „Satz“ erzeugt Einzelsätze (gut für Textklassifikation), „Fenster“ schneidet konfigurierbare Zeichenblöcke mit Überlappung.",
    example: "max_chars=600, overlap=50 → ≤600-Zeichen-Chunks mit 50 Zeichen Überlappung",
  },
  "train-data.annotation": {
    title: "Was macht die KI-Annotation?",
    body: "Ein Prompt (Aufgabe + erlaubte Labels) geht an den gewählten Provider (OpenAI-kompatibel: Mistral API, Ollama, LM Studio, llama.cpp …). Die Antwort {label, reasoning} wird als Entwurf gespeichert; im Audit-Trail steht der Actor ai:<Provider>. Nichts wird automatisch approved.",
    example: 'labels: ["navigation", "terminierung", "hilfe"]',
  },
  "train-data.review": {
    title: "Review-Workflow",
    body: "raw = Entwurf, curated = vom Menschen gesichtet, approved = export-bereit, rejected = ausgeschlossen. Wähle Zeilen an und nutze die Bulk-Aktionen.",
    example: "raw → curated → approved\n             ↘ rejected",
  },
  "train-data.split": {
    title: "Train / Dev / Eval",
    body: "train trainiert das spaCy-Modell, dev steuert Early Stopping, eval wird nur für den Schlussbericht gemessen. Der Konvertierungsschritt verteilt stratifiziert pro Label mit Seed.",
    example: "dev_ratio 0.2 → 80 % train, 20 % dev, pro Label gemischt",
  },
  "train-data.status": {
    title: "Item-Status",
    body: "raw frisch importiert, curated gesichtet, approved exportiert, rejected übersprungen. Nur curated + approved fließen in den Export.",
    example: "status=approved → in vorhandenem JSONL-Export",
  },
  "train-data.export": {
    title: "Warum JSONL für spaCy?",
    body: "spaCy trainiert vom DocBin-Format (.spacy). JSONL mit {text, label} ist das universelle Zwischenformat und wird von `spacy convert` akzeptiert.",
    example: "spacy convert export.jsonl out/ --lang de --converter json",
  },
  "train-data.labels": {
    title: "Label-Schema (kebab-case)",
    body: "Labels werden erzwungen in kebab-case (Kleinschreibung, Leerzeichen → „-“). Das hält ein konsistentes Label-Vokabular pro Datensatz, damit die Textklassifikations-Köpfe stabil trainieren.",
    example: '"Kundenservice Support" → kundenservice-support',
  },
  "train-data.providers": {
    title: "Annotation-Provider",
    body: "Verfügbar sind alle in Settings als OpenAI-kompatibel konfigurierten Endpunkte (Mistral API, Ollama, LM Studio, llama.cpp, vLLM …). Die Auswahl erscheint leer, bis in Settings mind. ein Provider angelegt ist.",
    example: "Settings → Providers → New provider (openai_compatible)",
  },
};

/** "?"-circle with hover/focus popup. Keyboard-accessible: focus shows the
 * panel (tab), Escape blurs; content is discoverable by screen readers via
 * aria-describedby. */
export default function HelpBubble({ helpId }: { helpId: string }) {
  const entry = helpTexts[helpId];
  if (!entry) return null;
  return (
    <span
      className="help-bubble"
      tabIndex={0}
      aria-label={`Hilfe: ${entry.title}`}
    >
      <span className="help-bubble-trigger" aria-hidden="true">
        ?
      </span>
      <span className="help-bubble-panel" role="tooltip">
        <b>{entry.title}</b>
        <p>{entry.body}</p>
        {entry.example && <pre className="help-bubble-example">{entry.example}</pre>}
      </span>
    </span>
  );
}
