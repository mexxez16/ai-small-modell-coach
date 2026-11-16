/* Global application css (tokens + chrome). */
import "./styles/tokens.css";
import "./styles/app.css";

import { useCallback, useEffect, useMemo, useState } from "react";
import { BrowserRouter, NavLink, Route, Routes, useNavigate } from "react-router-dom";

import Dashboard from "./pages/Dashboard";
import Sessions from "./pages/Sessions";
import Runs from "./pages/Runs";
import TrainingData from "./pages/TrainingData";
import Visualize from "./pages/Visualize";
import Images from "./pages/Images";
import Settings from "./pages/Settings";
import CommandPalette from "./components/CommandPalette";
import ChatDock from "./components/ChatDock";
import TerminalDock from "./components/TerminalDock";

const NAV_SECTIONS: Array<{ section: string; items: Array<{ to: string; label: string }> }> = [
  {
    section: "Overview",
    items: [
      { to: "/", label: "Dashboard" },
      { to: "/sessions", label: "Sessions" },
      { to: "/runs", label: "Runs" },
    ],
  },
  {
    section: "Data",
    items: [
      { to: "/training-data", label: "Training Data" },
      { to: "/visualize", label: "Visualize" },
      { to: "/images", label: "Images" },
    ],
  },
  {
    section: "Workbench",
    items: [{ to: "/settings", label: "Settings" }],
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

function Sidebar() {
  return (
    <nav className="sidebar" aria-label="Main navigation">
      <div className="sidebar-brand">
        <span className="sidebar-brand-mark" aria-hidden="true" />
        <span>Modell Coach</span>
      </div>
      {NAV_SECTIONS.map((group) => (
        <div key={group.section}>
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
      <Sidebar />
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
          <Route path="/sessions" element={<Sessions />} />
          <Route path="/runs" element={<Runs />} />
          <Route path="/training-data" element={<TrainingData />} />
          <Route path="/visualize" element={<Visualize />} />
          <Route path="/images" element={<Images />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </main>
      <ChatDock
        open={chatOpen}
        onClose={toggleChat}
        width={chatWidth}
        onResizeStart={startResizeChat}
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
