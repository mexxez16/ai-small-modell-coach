/* Global application css (tokens + chrome). */
import "./styles/tokens.css";
import "./styles/app.css";

import { useCallback, useEffect, useMemo, useState } from "react";
import { BrowserRouter, NavLink, Route, Routes, useNavigate } from "react-router-dom";

import Dashboard from "./pages/Dashboard";
import RunDetail from "./pages/RunDetail";
import Sessions from "./pages/Sessions";
import Runs from "./pages/Runs";
import ModelsPage from "./pages/Models";
import TrainingData from "./pages/TrainingData";
import DataSessions from "./pages/DataSessions";
import DataSessionDetail from "./pages/DataSessionDetail";
import DataRuns from "./pages/DataRuns";
import Visualize from "./pages/Visualize";
import Images from "./pages/Images";
import Settings from "./pages/Settings";
import Assistant from "./pages/Assistant";
import CommandPalette from "./components/CommandPalette";
import ChatDock from "./components/ChatDock";
import TerminalDock from "./components/TerminalDock";
import {
  ChatSession,
  CHATS_CHANGED_EVENT,
  deleteSession,
  domainColor,
  listSessions,
} from "./components/chatStore";

/** Canonical profile order for the CHATS list; unknown profiles trail. */
const PROFILE_ORDER = ["coach", "data", "finetuning"];

const NAV_SECTIONS: Array<{ section: string; items: Array<{ to: string; label: string }> }> = [
  {
    section: "Overview",
    items: [{ to: "/", label: "Dashboard" }],
  },
  {
    section: "Finetuning",
    items: [
      { to: "/sessions", label: "Sessions" },
      { to: "/runs", label: "Runs" },
      { to: "/models", label: "Models" },
    ],
  },
  {
    section: "Data",
    items: [
      { to: "/training-data/sessions", label: "Training Data Sessions" },
      { to: "/training-data/runs", label: "Training Data Runs" },
    ],
  },
  {
    section: "Visual Analysis",
    items: [
      { to: "/visualize", label: "Visualize" },
      { to: "/images", label: "Images" },
    ],
  },
  {
    section: "Settings",
    items: [
      { to: "/assistant", label: "Assistenten" },
      { to: "/settings", label: "General" },
    ],
  },
];

function useTheme(): [string, () => void] {
  const [theme, setTheme] = useState<string>(
    () => localStorage.getItem("coach.theme") ?? "dark",
  );
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("coach.theme", theme);
  }, [theme]);
  const toggle = useCallback(
    () => setTheme((t) => (t === "dark" ? "light" : "dark")),
    [],
  );
  return [theme, toggle];
}

function usePersistedToggle(key: string, initial: boolean): [boolean, () => void] {
  const [value, setValue] = useState<boolean>(() => {
    const stored = localStorage.getItem(key);
    return stored === null ? initial : stored === "true";
  });
  const toggle = useCallback(
    () =>
      setValue((v) => {
        localStorage.setItem(key, String(!v));
        return !v;
      }),
    [key],
  );
  return [value, toggle];
}

const CHAT_WIDTH_KEY = "coach.dock.chatWidth";
const TERMINAL_HEIGHT_KEY = "coach.dock.terminalHeight";
const CHAT_WIDTH = { min: 280, max: 640 };
const TERMINAL_HEIGHT = { min: 150, max: 600 };

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));

function usePersistedNumber(key: string, initial: number): [number, (n: number) => void] {
  const [value, setValue] = useState<number>(() => {
    const stored = Number(localStorage.getItem(key));
    return Number.isFinite(stored) && stored > 0 ? stored : initial;
  });
  const set = useCallback(
    (n: number) => {
      setValue(n);
      localStorage.setItem(key, String(n));
    },
    [key],
  );
  return [value, set];
}

function Sidebar({ onOpenChat }: { onOpenChat: (sessionId: string) => void }) {
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [now, setNow] = useState<number>(() => Date.now());
  const [filter, setFilter] = useState("");

  useEffect(() => {
    const reload = () => {
      setSessions(listSessions());
      setNow(Date.now());
    };
    reload();
    window.addEventListener(CHATS_CHANGED_EVENT, reload);
    return () => window.removeEventListener(CHATS_CHANGED_EVENT, reload);
  }, []);

  const sorted = [...sessions].sort((a, b) => {
    const orderDiff = PROFILE_ORDER.indexOf(a.profileId) - PROFILE_ORDER.indexOf(b.profileId);
    return orderDiff !== 0 || a.profileId !== b.profileId ? orderDiff : b.updatedAt - a.updatedAt;
  });
  const needle = filter.trim().toLowerCase();
  const visible = needle
    ? sorted.filter((s) => s.title.toLowerCase().includes(needle) || s.profileId.toLowerCase().includes(needle))
    : sorted;

  return (
    <nav className="sidebar" aria-label="Main navigation">
      <div className="sidebar-brand">
        <span>Small Model Coach</span>
      </div>
      {NAV_SECTIONS.map((group) => (
        <div key={group.section} data-domain={group.section.toLowerCase()}>
          <div className="sidebar-section">{group.section}</div>
          {group.items.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.to === "/"}
              className={({ isActive }) => `nav-link${isActive ? " is-active" : ""}`}
            >
              {item.label}
            </NavLink>
          ))}
        </div>
      ))}
      <div className="sidebar-chats">
        <div className="sidebar-section">Chats</div>
        <input
          className="text-input sidebar-chat-filter"
          placeholder="Chats filtern…"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        {visible.length === 0 && (
          <div className="sidebar-chat-item is-empty">
            {sessions.length === 0 ? "Noch keine Chats." : "Kein Treffer."}
          </div>
        )}
        {visible.map((session) => {
          const dotColor = domainColor(session.profileId, PROFILE_ORDER);
          const minutes = Math.max(0, Math.round((now - session.updatedAt) / 60000));
          return (
            <div key={session.id} className="sidebar-chat-item" data-profile={session.profileId}>
              <button
                className="sidebar-chat-open"
                onClick={() => onOpenChat(session.id)}
                title={`${session.profileId} · vor ${minutes < 1 ? "<1" : minutes} min`}
              >
                <span className="sidebar-chat-dot" style={{ background: dotColor }} />
                <span className="sidebar-chat-title">{session.title}</span>
              </button>
              <button
                className="sidebar-chat-delete"
                aria-label={`Chat löschen: ${session.title}`}
                onClick={() => deleteSession(session.id)}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>
    </nav>
  );
}

function TopBar({
  onOpenPalette,
  theme,
  toggleTheme,
  chatOpen,
  toggleChat,
  terminalOpen,
  toggleTerminal,
}: {
  onOpenPalette: () => void;
  theme: string;
  toggleTheme: () => void;
  chatOpen: boolean;
  toggleChat: () => void;
  terminalOpen: boolean;
  toggleTerminal: () => void;
}) {
  return (
    <header className="topbar">
      <button className="search-button" onClick={onOpenPalette}>
        Search or jump to… <kbd>Ctrl K</kbd>
      </button>
      <div className="topbar-right">
        <button
          className={`button ${chatOpen ? "button-primary" : "button-ghost"}`}
          onClick={toggleChat}
          aria-pressed={chatOpen}
        >
          Chat
        </button>
        <button
          className={`button ${terminalOpen ? "button-primary" : "button-ghost"}`}
          onClick={toggleTerminal}
          aria-pressed={terminalOpen}
        >
          Terminal
        </button>
        <button className="button button-ghost" onClick={toggleTheme} aria-label="Toggle color theme">
          {theme === "dark" ? "Light" : "Dark"}
        </button>
      </div>
    </header>
  );
}

function App() {
  const [theme, toggleTheme] = useTheme();
  const [paletteOpen, setPaletteOpen] = useState(false);
  // Dock visibility persists across reloads; default: chat open, terminal closed.
  const [chatOpen, toggleChat] = usePersistedToggle("coach.dock.chat", true);
  const [terminalOpen, toggleTerminal] = usePersistedToggle("coach.dock.terminal", false);
  const [chatWidth, setChatWidth] = usePersistedNumber(CHAT_WIDTH_KEY, 420);
  const [terminalHeight, setTerminalHeight] = usePersistedNumber(TERMINAL_HEIGHT_KEY, 300);
  // Sidebar CHATS click: bump counter forces an effect run even for repeats.
  const [chatDemand, setChatDemand] = useState<{ sessionId: string; n: number } | null>(null);
  const navigate = useNavigate();

  const startResizeChat = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      const startX = e.clientX;
      const startWidth = chatWidth;
      const move = (ev: PointerEvent) =>
        setChatWidth(clamp(startWidth + (startX - ev.clientX), CHAT_WIDTH.min, CHAT_WIDTH.max));
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [chatWidth, setChatWidth],
  );

  const startResizeTerminal = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      const startY = e.clientY;
      const startHeight = terminalHeight;
      const move = (ev: PointerEvent) =>
        setTerminalHeight(
          clamp(startHeight + (startY - ev.clientY), TERMINAL_HEIGHT.min, TERMINAL_HEIGHT.max),
        );
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [terminalHeight, setTerminalHeight],
  );

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const anyDock = chatOpen || terminalOpen;
  const items = useMemo(
    () =>
      NAV_SECTIONS.flatMap((g) => g.items)
        .map((item) => ({ label: item.label, action: () => navigate(item.to) }))
        .concat([
          { label: "Toggle chat dock", action: toggleChat },
          { label: "Toggle terminal dock", action: toggleTerminal },
        ]),
    [navigate, toggleChat, toggleTerminal],
  );

  return (
    <div
      className={`app-shell${anyDock ? " has-docks" : ""}`}
      data-chat-open={chatOpen}
      data-terminal-open={terminalOpen}
      style={
        {
          "--coach-dock-width": `${chatWidth}px`,
          "--coach-terminal-height": `${terminalHeight}px`,
        } as React.CSSProperties
      }
    >
      <Sidebar
        onOpenChat={(sessionId) => {
          if (!chatOpen) toggleChat();
          setChatDemand((d) => ({ sessionId, n: (d?.n ?? 0) + 1 }));
        }}
      />
      <TopBar
        onOpenPalette={() => setPaletteOpen(true)}
        theme={theme}
        toggleTheme={toggleTheme}
        chatOpen={chatOpen}
        toggleChat={toggleChat}
        terminalOpen={terminalOpen}
        toggleTerminal={toggleTerminal}
      />
      <main className="content">
        <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/runs/:runId" element={<RunDetail />} />
        <Route path="/sessions" element={<Sessions />} />
        <Route path="/runs" element={<Runs />} />
        <Route path="/models" element={<ModelsPage />} />
        <Route path="/training-data" element={<TrainingData mode="runs" />} />
        <Route path="/training-data/sessions" element={<DataSessions />} />
        <Route path="/training-data/sessions/:id" element={<DataSessionDetail />} />
        <Route path="/training-data/runs" element={<DataRuns />} />
        <Route path="/training-data/runs/:runId" element={<DataRuns />} />
        <Route path="/visualize" element={<Visualize />} />
        <Route path="/images" element={<Images />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="/assistant" element={<Assistant />} />
        </Routes>
      </main>
      <ChatDock
        open={chatOpen}
        onClose={toggleChat}
        width={chatWidth}
        onResizeStart={startResizeChat}
        demand={chatDemand}
      />
      <TerminalDock
        open={terminalOpen}
        onClose={toggleTerminal}
        onResizeStart={startResizeTerminal}
      />
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} items={items} />
    </div>
  );
}

export default function Root() {
  return (
    <BrowserRouter>
      <App />
    </BrowserRouter>
  );
}
