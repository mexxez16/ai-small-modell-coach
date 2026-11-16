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
import Chat from "./pages/Chat";
import Terminal from "./pages/Terminal";
import Settings from "./pages/Settings";
import CommandPalette from "./components/CommandPalette";

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
    items: [
      { to: "/chat", label: "Chat" },
      { to: "/terminal", label: "Terminal" },
      { to: "/settings", label: "Settings" },
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

function TopBar({ onOpenPalette, theme, toggleTheme }: {
  onOpenPalette: () => void;
  theme: string;
  toggleTheme: () => void;
}) {
  return (
    <header className="topbar">
      <button className="search-button" onClick={onOpenPalette}>
        Search or jump to… <kbd>Ctrl K</kbd>
      </button>
      <div className="topbar-right">
        <button className="button button-ghost" onClick={toggleTheme} aria-label="Toggle color theme">
          {theme === "dark" ? "Light" : "Dark"}
        </button>
      </div>
    </header>
  );
}

const PALETTE_ITEMS = NAV_SECTIONS.flatMap((g) => g.items).concat([
  { to: "/runs?query=", label: "New run query" },
]);

function App() {
  const [theme, toggleTheme] = useTheme();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const navigate = useNavigate();

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

  const items = useMemo(
    () => PALETTE_ITEMS.map((item) => ({ ...item, action: () => navigate(item.to) })),
    [navigate],
  );

  return (
    <div className="app-shell">
      <Sidebar />
      <TopBar onOpenPalette={() => setPaletteOpen(true)} theme={theme} toggleTheme={toggleTheme} />
      <main className="content">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/sessions" element={<Sessions />} />
          <Route path="/runs" element={<Runs />} />
          <Route path="/training-data" element={<TrainingData />} />
          <Route path="/visualize" element={<Visualize />} />
          <Route path="/images" element={<Images />} />
          <Route path="/chat" element={<Chat />} />
          <Route path="/terminal" element={<Terminal />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </main>
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
