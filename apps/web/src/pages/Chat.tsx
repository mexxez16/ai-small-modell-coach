import { useEffect, useRef, useState } from "react";

type ChatMessage = { role: "user" | "agent" | "system"; text: string; ts: string };

export default function Chat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [connected, setConnected] = useState(false);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${proto}://${location.host}/ws/chat`);
    socketRef.current = socket;
    socket.onopen = () => {
      setConnected(true);
      setMessages((m) => [
        ...m,
        {
          role: "system",
          text: "Connected to the Mistral Vibe bridge (scaffold: echo mode).",
          ts: new Date().toISOString(),
        },
      ]);
    };
    socket.onmessage = (event) => {
      setMessages((m) => [...m, { role: "agent", text: String(event.data), ts: new Date().toISOString() }]);
    };
    socket.onclose = () => setConnected(false);
    return () => socket.close();
  }, []);

  const send = () => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN || !input.trim()) return;
    setMessages((m) => [...m, { role: "user", text: input, ts: new Date().toISOString() }]);
    socket.send(input);
    setInput("");
  };

  return (
    <div style={{ display: "grid", gridTemplateRows: "auto 1fr auto", height: "100%" }}>
      <div className="page-header">
        <h1>Chat</h1>
        <p>
          Embedded Mistral Vibe agent, scoped to this workspace. Tool calls and
          approvals appear inline once the agent bridge is live.
        </p>
      </div>
      <div className="card" style={{ overflow: "auto" }}>
        {messages.map((message, index) => (
          <p key={index}>
            <span className={`badge badge-${message.role === "user" ? "neutral" : "success"}`}>
              {message.role}
            </span>{" "}
            {message.text}
          </p>
        ))}
        {messages.length === 0 && <p className="stat-label">No messages yet.</p>}
      </div>
      <div className="field-row" style={{ marginTop: 12 }}>
        <input
          className="text-input"
          style={{ flex: 1 }}
          placeholder={connected ? "Ask the agent…" : "Waiting for the agent bridge…"}
          value={input}
          disabled={!connected}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && send()}
        />
        <button className="button button-primary" onClick={send} disabled={!connected}>
          Send
        </button>
      </div>
    </div>
  );
}
