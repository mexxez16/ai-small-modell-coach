import { useEffect, useRef } from "react";

/**
 * Terminal: xterm.js frontend over the `/ws/terminal` PTY socket.
 *
 * The scaffold intentionally avoids the xterm.js dependency until the
 * PTY feature lands; this shell renders the echo-protocol smoke path using
 * websocket text frames so wiring is testable end to end.
 */
export default function Terminal() {
  const outRef = useRef<HTMLPreElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${proto}://${location.host}/ws/terminal`);
    socketRef.current = socket;
    const append = (line: string) => {
      if (outRef.current) {
        outRef.current.textContent += line + "\n";
      }
    };
    socket.onopen = () => append("Connected to PTY echo (scaffold).");
    socket.onmessage = (event) => append(String(event.data));
    socket.onclose = () => append("Disconnected.");
    return () => socket.close();
  }, []);

  const send = () => {
    const socket = socketRef.current;
    if (socket?.readyState === WebSocket.OPEN && inputRef.current?.value) {
      socket.send(inputRef.current.value);
      inputRef.current.value = "";
    }
  };

  return (
    <div>
      <div className="page-header">
        <h1>Terminal</h1>
        <p>PTY-backed terminal tabs scoped to the workspace directory.</p>
      </div>
      <div className="card">
        <pre
          ref={outRef}
          style={{
            minHeight: 200,
            maxHeight: 380,
            overflow: "auto",
            background: "var(--color-bg)",
            padding: 12,
            borderRadius: 8,
            fontFamily: "var(--font-mono)",
          }}
        />
        <div className="field-row">
          <input
            ref={inputRef}
            className="text-input"
            style={{ flex: 1, fontFamily: "var(--font-mono)" }}
            placeholder="Command (echo mode in scaffold)"
            onKeyDown={(event) => event.key === "Enter" && send()}
          />
        </div>
      </div>
    </div>
  );
}
