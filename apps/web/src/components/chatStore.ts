/**
 * Local chat session store: the dock keeps its live thread in React state,
 * everything worth keeping after a turn lands here (localStorage) so the
 * sidebar can list chats and the dock can reopen them after a reload.
 * Persistence is deliberately frontend-only: chats are working notes, not
 * app data that must survive a browser reset.
 */

export type ChatMessage = { role: "user" | "assistant"; text: string };

export type ChatSession = {
  id: string;
  profileId: string;
  title: string;
  updatedAt: number;
  messages: ChatMessage[];
};

const KEY = "coach.chats";
export const CHATS_CHANGED_EVENT = "coach.chats.changed";

/** Stable per-assistant hue shared by dock and sidebar. */
export const PROFILE_TOKENS: Record<string, string> = {
  coach: "var(--color-domain-coach)",
  data: "var(--color-domain-data)",
  finetuning: "var(--color-domain-finetuning)",
};
export const PROFILE_FALLBACK = ["#7048e8", "#0c8599", "#d9480f"];

export function domainColor(id: string | undefined, order: string[] = []): string {
  if (!id) return "#8a8f98";
  if (PROFILE_TOKENS[id]) return PROFILE_TOKENS[id];
  const index = order.indexOf(id);
  return PROFILE_FALLBACK[(index >= 0 ? index : 0) % PROFILE_FALLBACK.length];
}

export function listSessions(): ChatSession[] {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as ChatSession[]) : [];
    return Array.isArray(parsed) ? parsed.filter((s) => s && s.id && s.profileId) : [];
  } catch {
    return [];
  }
}

export function latestSession(profileId: string): ChatSession | null {
  const sessions = listSessions().filter((s) => s.profileId === profileId);
  if (sessions.length === 0) return null;
  return sessions.reduce((a, b) => (b.updatedAt > a.updatedAt ? b : a));
}

export function upsertSession(session: ChatSession): void {
  const sessions = listSessions().filter((s) => s.id !== session.id);
  sessions.push(session);
  localStorage.setItem(KEY, JSON.stringify(sessions));
  window.dispatchEvent(new Event(CHATS_CHANGED_EVENT));
}

export function deleteSession(id: string): void {
  localStorage.setItem(KEY, JSON.stringify(listSessions().filter((s) => s.id !== id)));
  window.dispatchEvent(new Event(CHATS_CHANGED_EVENT));
}

export function newSessionId(): string {
  return typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `chat-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
}

export function titleFor(text: string): string {
  const clean = text.replace(/\s+/g, " ").trim();
  return clean.length > 40 ? `${clean.slice(0, 40)}…` : clean || "Leerer Chat";
}
