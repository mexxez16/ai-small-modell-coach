import { useMemo, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";

export type PaletteItem = {
  label: string;
  action: () => void;
};

export default function CommandPalette({
  open,
  onOpenChange,
  items,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: PaletteItem[];
}) {
  const [filter, setFilter] = useState("");
  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return q ? items.filter((i) => i.label.toLowerCase().includes(q)) : items;
  }, [filter, items]);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="overlay" />
        <Dialog.Content
          className="dialog"
          aria-describedby={undefined}
          onKeyDown={(event) => {
            if (event.key === "Escape") onOpenChange(false);
          }}
        >
          <Dialog.Title style={{ margin: "0 0 12px" }}>Jump to</Dialog.Title>
          <input
            className="text-input"
            autoFocus
            placeholder="Type a page name…"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
          <div className="command-list">
            {filtered.map((item) => (
              <button
                key={item.label}
                className="command-item"
                onClick={() => {
                  item.action();
                  onOpenChange(false);
                  setFilter("");
                }}
              >
                {item.label}
              </button>
            ))}
            {filtered.length === 0 && <div className="empty-state">No matches.</div>}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
