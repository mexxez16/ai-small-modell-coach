import { useCallback, useEffect, useRef, useState } from "react";

import { Provider } from "../api/client";

type ChatFrame =
  | { type: "system"; message?: string; providers?: Provider[] }
  | { type: "delta"; content: string }
  | { type: "done" }
  | { type: "error"; message: string };

type ChatMessage = { role: "user" | "assistant" | "system"; text: string };

/** Strip terminal control sequences; headless providers should never emit
 * them, but raw ANSI in the chat list is unreadable if one slips through. */
function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b(?:\[[0-9;?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\)|[@-Z\\-_])/g, "");
}

/**
 * Chat dock: right-side panel, collapsible. Connects to /ws/chat; the
 * provider dropdown is fed by the provider list the backend announces on
 * connect (same list as GET /api/settings/providers).
 */
export default function ChatDock({
  open,
  onClose,
  width,
  onResizeStart,
}: {
  open: boolean;
  onClose: () => void;
  width: number;
  onResizeStart: (e: React.PointerEvent) => void;
}) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [providerId, setProviderId] = useState<string>("");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [status, setStatus] = useState<"connecting" | "connected" | "closed">("connecting");
  const [connNonce, setConnNonce] = useState(0);
  const socketRef = useRef<WebSocket | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const append = useCallback((message: ChatMessage) => {
    setMessages((m) => [...m, message]);
  }, []);

  // Consecutive delta frames extend the last assistant message instead of
  // creating one bubble per chunk.
  const appendDelta = useCallback((content: string) => {
    setMessages((m) => {
      const last = m[m.length - 1];
      if (last?.role === "assistant") {
        return [...m.slice(0, -1), { role: "assistant", text: last.text + content }];
      }
      return [...m, { role: "assistant", text: content }];
    });
  }, []);

  const refreshProviders = useCallback(() => {
    const socket = socketRef.current;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: "providers" }));
    } else if (!socket || socket.readyState === WebSocket.CLOSED || socket.readyState === WebSocket.CLOSING) {
      // Stale/dead socket: re-open the connection.
      setConnNonce((n) => n + 1);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    // Providers may be added in Settings while the chat is open; re-fetch the
    // list when the user returns to the dock.
    window.addEventListener("focus", refreshProviders);
    window.addEventListener("coach.providers-changed", refreshProviders);
    return () => {
      window.removeEventListener("focus", refreshProviders);
      window.removeEventListener("coach.providers-changed", refreshProviders);
    };
  }, [open, refreshProviders]);

  useEffect(() => {
    if (!open) return;
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${proto}://${location.host}/ws/chat`);
    socketRef.current = socket;
    setStatus("connecting");

    const currentProvider = localStorage.getItem("coach.chatProvider") ?? "";

    socket.onopen = () => setStatus("connected");
    socket.onclose = () => setStatus("closed");
    socket.onerror = () => setStatus("closed");
    socket.onmessage = (event) => {
      let frame: ChatFrame;
      try {
        frame = JSON.parse(String(event.data));
      } catch {
        return;
      }
      switch (frame.type) {
        case "system": {
          if (frame.providers) {
            setProviders(frame.providers);
            setProviderId((current) => {
              const stillThere = frame.providers!.some((p) => p.id === current);
              if (stillThere) return current;
              const next = frame.providers!.find((p) => p.id === currentProvider) ?? frame.providers![0];
              return next?.id ?? "";
            });
          }
          if (frame.message) append({ role: "system", text: frame.message });
          break;
        }
        case "delta":
          appendDelta(stripAnsi(frame.content));
          break;
        case "done":
          break;
        case "error":
          append({ role: "system", text: `Error: ${frame.message}` });
          break;
      }
    };

    return () => socket.close();
  }, [open, connNonce, append, appendDelta]);

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [messages]);

  const send = () => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN || !input.trim() || !providerId) return;
    append({ role: "user", text: input.trim() });
    socket.send(JSON.stringify({ type: "chat", provider_id: providerId, text: input }));
    setInput("");
  };

  if (!open) return null;

  return (
    <aside className="dock-chat" aria-label="Chat dock" style={{ width }}>
      <div
        className="resize-handle resize-handle-x"
        onPointerDown={onResizeStart}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize chat dock"
      />
      <header className="dock-header">
        <h3>Chat</h3>
        <select
          className="select"
          value={providerId}
          onChange={(event) => {
            const id = event.target.value;
            setProviderId(id);
            localStorage.setItem("coach.chatProvider", id);
          }}
          aria-label="Chat provider"
        >
          <option value="">— select provider —</option>
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.name}
            </option>
          ))}
        </select>
        <span className={`badge badge-${status === "connected" ? "success" : status === "connecting" ? "warning" : "danger"}`}>
          {status}
        </span>
        <button className="button button-ghost" onClick={refreshProviders} aria-label="Refresh provider list">
          Refresh
        </button>
        <button className="button button-ghost" onClick={onClose} aria-label="Close chat dock">
          Hide
        </button>
      </header>

      {providers.length === 0 && status === "connected" && (
        <div className="empty-state" style={{ margin: "12px" }}>
          <h3>No providers configured</h3>
          <p>Add one in Settings → Chat providers (Mistral API, Ollama, LM Studio, llama.cpp, …) or use Vibe.</p>
        </div>
      )}

      <div className="chat-messages" ref={listRef}>
        {messages.map((message, index) => (
          <p key={index}>
            <span className={`badge badge-${message.role === "user" ? "neutral" : message.role === "assistant" ? "success" : "warning"}`}>
              {message.role}
            </span>{" "}
            {message.text}
          </p>
        ))}
        {messages.length === 0 && (
          <p className="stat-label" style={{ padding: "12px" }}>
            Ask a provider something — streaming output appears here.
          </p>
        )}
      </div>

      <div className="dock-input-row">
        <input
          className="text-input"
          style={{ flex: 1 }}
          placeholder={providerId ? "Message…" : "Select a provider first"}
          value={input}
          disabled={!providerId || status !== "connected"}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && send()}
        />
        <button className="button button-primary" onClick={send} disabled={!providerId || status !== "connected"}>
          Send
        </button>
      </div>
    </aside>
  );
}
