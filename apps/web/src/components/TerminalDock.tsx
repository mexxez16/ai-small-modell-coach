import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";

import "@xterm/xterm/css/xterm.css";

/**
 * Terminal dock: bottom panel attached below the chat dock, collapsible. One
 * real PTY per connection (scoped to the workspace dir); xterm.js renders the
 * raw VT stream and sends raw keystrokes as binary frames.
 */
export default function TerminalDock({
  open,
  onClose,
  onResizeStart,
}: {
  open: boolean;
  onClose: () => void;
  onResizeStart: (e: React.PointerEvent) => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<{ term: Terminal; fit: FitAddon; socket: WebSocket } | null>(null);

  useEffect(() => {
    if (!open || !hostRef.current || termRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: "var(--font-mono)",
      theme: document.documentElement.dataset.theme === "dark"
        ? { background: "#10141b", foreground: "#e8edf4" }
        : { background: "#ffffff", foreground: "#1a212b" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(hostRef.current);

    const proto = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${proto}://${location.host}/ws/terminal?cols=80&rows=24`);
    socket.binaryType = "arraybuffer";

    socket.onopen = () => {
      const { cols, rows } = term;
      socket.send(JSON.stringify({ type: "resize", cols, rows }));
      term.onResize(({ cols, rows }) =>
        socket.send(JSON.stringify({ type: "resize", cols, rows })),
      );
      fit.fit();
    };
    socket.onmessage = (event) => {
      if (typeof event.data === "string") {
        try {
          const frame = JSON.parse(event.data);
          if (frame.type === "exit") term.write("\r\n\x1b[2m[shell exited]\x1b[0m\r\n");
          return;
        } catch {
          // plain text line-mode reply
        }
        term.write(event.data);
        return;
      }
      term.write(new Uint8Array(event.data));
    };
    socket.onclose = () => term.write("\r\n\x1b[2m[disconnected]\x1b[0m\r\n");
    term.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) {
        // xterm sends single keystrokes/paste blobs — wire mode.
        socket.send(new TextEncoder().encode(data));
      }
    });

    termRef.current = { term, fit, socket };

    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(hostRef.current);

    return () => {
      observer.disconnect();
      socket.close();
      term.dispose();
      termRef.current = null;
    };
  }, [open]);

  if (!open) return null;

  return (
    <section className="dock-terminal" aria-label="Terminal dock">
      <div
        className="resize-handle resize-handle-y"
        onPointerDown={onResizeStart}
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize terminal dock"
      />
      <div className="dock-header">
        <h3>Terminal</h3>
        <span className="badge badge-neutral">workspace shell</span>
        <button className="button button-ghost" onClick={onClose} aria-label="Close terminal dock">
          Hide
        </button>
      </div>
      <div ref={hostRef} className="terminal-host" />
    </section>
  );
}
