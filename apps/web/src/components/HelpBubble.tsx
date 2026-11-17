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
    body: "Eine Quelle ist ein hochgeladenes Dokument (Text, Markdown, JSON). Sie wird mit sha256 gespeichert und ist die Basis für das Chunking — nichts landet ohne nachvollziehbare Herkunft im Datensatz.",
    example: 'POST /api/sources  { name: "meine-notizen.txt", content: "..." }',
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
  "models.register": {
    title: "Modell registrieren",
    body: "Name + Pfad eines spaCy-Pipeline-Ordners (mit meta.json + config.cfg) oder eines installierten Hub-Pakets (z.B. en_core_web_sm). Registrierte Modelle erscheinen als Auswahloption beim Erstellen von Runs.",
    example: "pfad: training/output/model-best",
  },
  "data-runs.pipeline": {
    title: "Was ist ein Data-Run?",
    body: "Ein Data-Run ist eine frei konfigurierbare Pipeline in sechs Abschnitten: (1) Datenquelle, (2) Vorverarbeitung wie Chunking, (3) gewünschte Beispiel-Typen, (4) KI-Verarbeitung mit bearbeitbarem Prompt, (5) optionaler menschlicher Check, (6) Ausgabe mit Tabellen und Kennzahlen. Jede Ausführung schreibt Kennzahlen und übersprungene Datensätze ins Run-Ergebnis.",
    example: "SSOT-Datei → Absatz-Chunking → KI-Annotation → Review → JSONL/Dataset fürs Finetuning",
  },
  "pipeline.input": {
    title: "Datenquelle wählen",
    body: "Die Quelle ist ein hochgeladenes Dokument. SSOT-JSON mit 'entities' und 'relations' wird strukturiert zerlegt; andere Texte brauchen das Chunking aus Abschnitt 2. Quellen entstehen im Workbench-Bereich über Datei-Upload oder direktes Einfügen.",
    example: 'entities: {"Begriff": {examples: [...], negative_examples: [...]}}',
  },
  "pipeline.preprocessing": {
    title: "Vorverarbeitung (Chunking)",
    body: "Der deterministische Chunker zerlegt Text reproduzierbar: Absätze bewahren thematische Blöcke, Einzelzätze sind ideal für Textklassifikation, das Fenster schneidet feste Blöcke mit Überlappung. Kein LLM nötig — kostenlos und nachvollziehbar.",
    example: "max_chars=600, overlap=50 → ≤600-Zeichen-Chunks mit 50 Zeichen Überlappung",
  },
  "pipeline.output_settings": {
    title: "Beispiel-Typen",
    body: "Welche Typen am Ende exportiert werden: examples (normale Klassifikationsbeispiele), negative (bewusst falsche/keine-Entität), contrastive (Grenzfälle zum Vergleich) und Tripel-Paare (Subjekt—Prädikat—Objekt) für Relations-Extraktion. Die Wahl steuert die Export-Heads gleichermaßen.",
    example: "type.head.jsonl · relevance.head.jsonl · relation.head.jsonl",
  },
  "pipeline.processing": {
    title: "KI-Verarbeitung & Prompt",
    body: "Jeder konfigurierte Provider (Mistral API, Ollama, LM Studio …) kann hier die Items klassifizieren. Der Prompt ist voll editierbar: {labels} und {text} sind Platzhalter für die Label-Liste bzw. den Item-Text. Ergebnisse landen immer als Entwurf.",
    example: '{"label": "material-entity", "reasoning": "..."}',
  },
  "pipeline.prompt": {
    title: "Prompt-Platzhalter",
    body: "{labels} wird durch die erlaubten Labels ersetzt, {text} durch den Item-Text. Wenn du die Vorlage leer lässt, gilt der eingebaute Standard-Prompt (Label + kurze Begründung als JSON).",
    example: "Erlaubte Labels:\n{labels}\n\nText:\n{text}",
  },
  "pipeline.curation": {
    title: "Human Check",
    body: "Wenn aktiv, bleiben alle KI-Antworten im Status raw und müssen gesichtet werden (raw → curated → approved, oder rejected). „Einzeln nach Typ“ gruppiert den Review-Stapel nach Beispiel-Typ, statt alles gemischt zu zeigen.",
    example: "raw → curated → approved\n             ↘ rejected",
  },
  "pipeline.output": {
    title: "Ausgabe eines Runs",
    body: "Nach der Ausführung findest du hier je Stage die Kennzahlen (hinzugefügt, übersprungen, Klassen) und Links: JSONL-Export und Workbench zur Sichtung der erzeugten Items. Das Dataset taucht danach als wählbarer Datensatz in Finetuning-Runs auf.",
    example: "stage_0_ssot_import: added=42 · stage_3_export: heads=3",
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
