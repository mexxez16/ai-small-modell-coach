import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, AssistantProfile, AssistantStreamEvent } from "../api/client";

type ChatMessage = { role: "user" | "assistant"; text: string };
type Activity = { kind: "line" | "handoff_card"; text: string; profile?: string; reason?: string; opening?: string };

/** Stable per-assistant hue (design tokens) so chats are visually attributable. */
const PROFILE_TOKENS: Record<string, string> = {
  coach: "var(--color-domain-coach)",
  data: "var(--color-domain-data)",
  finetuning: "var(--color-domain-finetuning)",
};
const PROFILE_FALLBACK = ["#7048e8", "#0c8599", "#d9480f"];
const NEUTRAL_COLOR = "#8a8f98";
/** Translucent wash of a domain hue, theme-aware via color-mix. */
const tint = (color: string, share = 14) => `color-mix(in srgb, ${color} ${share}%, transparent)`;

/**
 * Chat dock: right-side panel, collapsible. Assistant hub with three
 * profiles (Small Model Coach / Data / Finetuning); each profile owns its
 * own thread, prompt, tools and starter chips — all configured in the
 * Workbench. Turns stream NDJSON events from POST /api/assistant/turn.
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
  const [profiles, setProfiles] = useState<AssistantProfile[]>([]);
  const [compatibleProviders, setCompatibleProviders] = useState<Array<{ id: string; name: string }>>([]);
  const [providerId, setProviderId] = useState<string>(() => localStorage.getItem("coach.assistantProvider") ?? "");
  const [threadsByProfile, setThreadsByProfile] = useState<Record<string, ChatMessage[]>>({});
  const [activityByProfile, setActivityByProfile] = useState<Record<string, Activity[]>>({});
  const [profileId, setProfileId] = useState<string>("");
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  const activeProfile = profiles.find((p) => p.id === profileId);
  const messages = useMemo(() => threadsByProfile[profileId] ?? [], [threadsByProfile, profileId]);
  const activity = useMemo(() => activityByProfile[profileId] ?? [], [activityByProfile, profileId]);

  const profileColor = (id: string): string => {
    if (PROFILE_TOKENS[id]) return PROFILE_TOKENS[id];
    const index = profiles.findIndex((p) => p.id === id);
    return index >= 0 ? PROFILE_FALLBACK[index % PROFILE_FALLBACK.length] : NEUTRAL_COLOR;
  };
  const color = profileId ? profileColor(profileId) : NEUTRAL_COLOR;
  const targetColor = (id: string | undefined): string => (id ? profileColor(id) : NEUTRAL_COLOR);

  const setThread = (profile: string, thread: ChatMessage[] | ((prev: ChatMessage[]) => ChatMessage[])) =>
    setThreadsByProfile((all) => ({ ...all, [profile]: typeof thread === "function" ? thread(all[profile] ?? []) : thread }));

  const setActivityLines = (profile: string, lines: Activity[] | ((prev: Activity[]) => Activity[])) =>
    setActivityByProfile((all) => ({ ...all, [profile]: typeof lines === "function" ? lines(all[profile] ?? []) : lines }));

  const refresh = useCallback(async () => {
    try {
      const [res, provs] = await Promise.all([
        api.assistantProfiles(),
        api.listProviders(),
      ]);
      setProfiles(res.profiles);
      setCompatibleProviders(provs.providers.filter((p) => p.kind === "openai_compatible").map((p) => ({ id: p.id, name: p.name })));
      setProfileId((current) => {
        if (current && res.profiles.some((p) => p.id === current)) return current;
        // Fall back to the previous provider-style key, then to `coach`.
        const legacy = localStorage.getItem("coach.chatProvider") ?? "";
        const remembered = localStorage.getItem("coach.assistantProfile") ?? legacy;
        return res.profiles.some((p) => p.id === remembered) ? remembered : res.profiles[0]?.id ?? "";
      });
    } catch (err) {
      setActivityLines(profileId, [{ kind: "line", text: err instanceof Error ? err.message : String(err) }]);
    }
  }, [profileId]);

  useEffect(() => {
    if (!open) return;
    void refresh();
    // Intentionally only on open; profile switches refresh explicitly below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const switchProfile = (id: string, prefill?: string) => {
    setProfileId(id);
    localStorage.setItem("coach.assistantProfile", id);
    setActivityLines(id, []);
    if (prefill) setInput(prefill);
  };

  const confirmHandoff = (card: NonNullable<Activity["kind"] extends never ? never : Activity> & { profile: string }) => {
    const target = card.profile;
    switchProfile(target, card.opening || "");
  };

  const send = async () => {
    const text = input.trim();
    if (!text || busy || !profileId) return;
    const profile = profileId;
    const history = threadsByProfile[profile] ?? [];
    const next = [...history, { role: "user" as const, text }];
    setThread(profile, next);
    setInput("");
    setBusy(true);
    setActivityLines(profile, []);
    let finalReply = "";
    try {
      await api.assistantTurn(
        profileId,
        next.map((m) => ({ role: m.role, content: m.text })),
        (event: AssistantStreamEvent) => {
        switch (event.type) {
          case "tool":
            setActivityLines(profile, (lines) => [
              ...lines,
              { kind: "line", text: `Runde ${event.round}: ${event.tool}${"query" in event && event.query ? ` (${event.query})` : ""} — ${event.ok ? "erledigt" : "fehlgeschlagen"}` },
            ]);
            break;
          case "thinking":
            if (event.text.trim()) {
              setActivityLines(profile, (lines) => [...lines, { kind: "line", text: event.text.trim() }]);
            }
            break;
          case "handoff":
            setActivityLines(profile, (lines) => [
              ...lines,
              { kind: "handoff_card", text: event.opening || "Der Assistent schlägt eine Übergabe vor.", profile: event.profile, reason: event.reason, opening: event.opening },
            ]);
            break;
          case "final":
            finalReply = event.reply;
            break;
        }
        },
        providerId,
      );
      setThread(profile, (prev) => [...prev, { role: "assistant", text: finalReply || "(leere Antwort)" }]);
      if (finalReply.includes("Vorschläge hinterlegt")) void refresh();
    } catch (err) {
      setActivityLines(profile, [{ kind: "line", text: err instanceof Error ? err.message : String(err) }]);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [messages, activity]);

  if (!open) return null;

  return (
    <aside className="dock-chat" aria-label="Assistent dock" style={{ width }}>
      <div
        className="resize-handle resize-handle-x"
        onPointerDown={onResizeStart}
        role="separator"
        aria-orientation="vertical"
        aria-label="Assistent dock resize"
      />
      <header className="dock-header">
        <h3>Assistent</h3>
        <span className="badge" style={{ background: tint(color), color: "var(--color-text)" }}>{activeProfile?.name ?? "…"}</span>
        <select
          className="select"
          aria-label="Modell-Provider"
          title="Modell-Provider für den Assistenten"
          value={providerId}
          disabled={busy}
          onChange={(event) => {
            setProviderId(event.target.value);
            localStorage.setItem("coach.assistantProvider", event.target.value);
          }}
        >
          <option value="">Standard (Settings)</option>
          {compatibleProviders.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <button className="button button-ghost" onClick={onClose} aria-label="Close assistant dock">
          Hide
        </button>
      </header>

      {compatibleProviders.length === 0 && (
        <p className="stat-label" style={{ padding: "8px 12px 0", color: "var(--color-warning)" }}>
          Kein OpenAI-kompatibler Provider vorhanden — unter Settings → General einen Provider anlegen und als Standard setzen.
        </p>
      )}

      <div className="dock-profile-tabs">
        {profiles.map((profile) => {
          const tabColor = profileColor(profile.id);
          const active = profile.id === profileId;
          return (
            <button
              key={profile.id}
              className={`button dock-profile-tab ${active ? "is-active" : "button-ghost"}`}
              title={profile.description}
              onClick={() => switchProfile(profile.id)}
              style={
                active
                  ? { background: tint(tabColor), borderColor: tabColor }
                  : { borderLeft: `3px solid ${tabColor}` }
              }
            >
              <span className="dock-profile-dot" style={{ background: tabColor }} />
              {profile.name}
            </button>
          );
        })}
      </div>

      {!!activeProfile?.starter_prompts.length && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", padding: "8px 12px 0" }}>
          {activeProfile.starter_prompts.map((prompt, index) => (
            <button
              key={index}
              className="button button-ghost"
              style={{ fontSize: "var(--text-xs)" }}
              onClick={() => setInput(prompt)}
            >
              {prompt}
            </button>
          ))}
        </div>
      )}

      <div className="chat-messages" ref={listRef} style={{ borderLeft: `3px solid ${color}` }}>
        {messages.map((message, index) => (
          <p key={index}>
            {message.role === "assistant" ? (
              <span className="badge" style={{ background: tint(color), color: "var(--color-text)" }}>assistant</span>
            ) : (
              <span className="badge badge-neutral">user</span>
            )}{" "}
            {message.text}
          </p>
        ))}
        {!busy && activity.map((entry, index) =>
          entry.kind === "handoff_card" ? (
            <div key={`c${index}`} style={{ border: `1px solid ${targetColor(entry.profile)}`, borderRadius: 8, padding: 10, margin: 8 }}>
              <p style={{ margin: 0 }}>
                <b>Übergabe an {entry.profile}</b>
              </p>
              {entry.reason && <p className="stat-label" style={{ margin: "4px 0" }}>{entry.reason}</p>}
              {entry.opening && <p className="stat-label" style={{ margin: "4px 0" }}>Einstiegsfrage: <i>{entry.opening}</i></p>}
              <button className="button button-primary" onClick={() => confirmHandoff(entry as typeof entry & { profile: string })}>
                Weiterleiten
              </button>
            </div>
          ) : (
            <p key={`a${index}`} className="stat-label" style={{ padding: "0 12px", margin: "2px 0" }}>
              {entry.text}
            </p>
          ),
        )}
        {busy && <p className="stat-label" style={{ padding: "0 12px" }}>Assistent denkt…</p>}
        {messages.length === 0 && activity.length === 0 && (
          <p className="stat-label" style={{ padding: "12px" }}>
            {activeProfile
              ? `Frag den ${activeProfile.name}-Assistenten — Startpunkte oben.`
              : "Assistenten konfiguriertes Profil lädt…"}
          </p>
        )}
      </div>

      <div className="dock-input-row">
        <input
          className="text-input"
          style={{ flex: 1 }}
          placeholder={profileId ? "Nachricht…" : "Profil lädt…"}
          value={input}
          disabled={!profileId || busy}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void send()}
        />
        <button className="button button-primary" onClick={() => void send()} disabled={!profileId || busy}>
          Senden
        </button>
      </div>
    </aside>
  );
}
