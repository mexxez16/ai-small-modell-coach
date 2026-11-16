/**
 * Typed client for the coach-api REST surface.
 *
 * This module is the frontend-facing API surface; it is kept in sync with the
 * OpenAPI document served by coach-api at `/api/openapi.json` (see
 * `scripts/codegen.sh`, which regenerates `api/schema.d.ts` via
 * openapi-typescript). The scaffold hand-wires the calls; the generated
 * schema is the contract it must match.
 */

export type Session = {
  id: string;
  name: string;
  description: string;
  status: "active" | "archived";
  created_at: string;
  updated_at: string;
};

export type Run = {
  id: string;
  name: string;
  session_id?: string | null;
  status: "running" | "completed" | "failed";
  config?: unknown | null;
  hparams: unknown;
  created_at: string;
  updated_at: string;
  tags: string[];
};

export type RenderWorker = {
  id: string;
  model_path: string;
  port: number;
  pid?: number | null;
  status: "starting" | "ready" | "unhealthy" | "stopped";
  started_at: string;
};

export type Provider = {
  id: string;
  name: string;
  kind: "openai_compatible" | "vibe";
  base_url?: string | null;
  api_key?: string | null; // masked in list responses
  model?: string | null;
};

export type VisualizeRequest = {
  worker_id?: string;
  model?: string;
  text: string;
  style: "ent" | "dep" | "span";
  options?: Record<string, unknown>;
  sentence_split?: boolean;
};

export type VisualizeResponse = {
  svg: string;
  html?: string;
  meta: Record<string, unknown>;
};

export type Visualization = {
  id: string;
  run_id?: string | null;
  session_id?: string | null;
  worker_model: string;
  text?: string | null;
  dataset_item_id?: string | null;
  style: string;
  options: unknown;
  svg: string;
  created_at: string;
};

export type Health = { status: string; version: string };

export type DashboardData = {
  counts: {
    sessions: number;
    runs_total: number;
    runs_running: number;
    runs_completed: number;
    runs_failed: number;
    datasets: number;
    data_items: number;
    images: number;
    visualizations: number;
  };
  recent_runs: Run[];
  metric_names: string[];
  params: Array<{ id: string; name: string; status: string; hparams: unknown }>;
};

const BASE = import.meta.env.VITE_API_BASE ?? "";

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      const json = (await res.json()) as { error?: string };
      if (json.error) detail = json.error;
    } catch {
      /* statusText fallback */
    }
    throw new ApiError(res.status, detail);
  }
  // 204/empty bodies (e.g. DELETE) must not be parsed as JSON.
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export const api = {
  health: () => request<Health>("GET", "/api/health"),
  dashboard: () => request<DashboardData>("GET", "/api/dashboard"),
  listSessions: () => request<Session[]>("GET", "/api/sessions"),
  createSession: (name: string, description = "") =>
    request<Session>("POST", "/api/sessions", { name, description }),
  listRuns: (sessionId?: string) =>
    request<Run[]>(
      "GET",
      sessionId ? `/api/runs?session_id=${encodeURIComponent(sessionId)}` : "/api/runs",
    ),
  createRun: (name: string, sessionId?: string) =>
    request<Run>("POST", "/api/runs", { name, session_id: sessionId, hparams: {} }),
  queryRuns: (query: string) =>
    request<{ parsed: string; matched_runs: Run[]; note: string }>("POST", "/api/query", {
      query,
    }),
  listWorkers: () => request<RenderWorker[]>("GET", "/api/workers"),
  spawnWorker: (modelPath: string) =>
    request<{ worker: RenderWorker }>("POST", "/api/workers", { model_path: modelPath }),
  workerHealth: (id: string) =>
    request<{ id: string; status: string }>("GET", `/api/workers/${id}/health`),
  stopWorker: (id: string) => request<void>("DELETE", `/api/workers/${id}`),
  listProviders: () =>
    request<{ providers: Provider[] }>("GET", "/api/settings/providers"),
  upsertProvider: (provider: Partial<Provider> & { name: string; kind: Provider["kind"] }) =>
    request<{ provider: Provider }>("POST", "/api/settings/providers", provider),
  deleteProvider: (id: string) =>
    request<void>("DELETE", `/api/settings/providers/${id}`),
  testProvider: (id: string) =>
    request<{ ok: boolean; detail: string }>(
      "POST",
      `/api/settings/providers/${id}/test`,
      {},
    ),
  /** Proxied render endpoint; response size/content-type are validated server side. */
  visualize: (req: VisualizeRequest) =>
    request<VisualizeResponse>("POST", "/api/visualize", req),
  /** Raster export (PNG). Only style="dep" yields SVG the worker can rasterize. */
  visualizePng: (req: VisualizeRequest) =>
    request<{ png_base64: string; meta: Record<string, unknown> }>(
      "POST",
      "/api/visualize_png",
      req,
    ),
  listVisualizations: () => request<Visualization[]>("GET", "/api/visualizations"),
  saveVisualization: (payload: {
    run_id?: string;
    worker_model: string;
    text?: string;
    style: string;
    options?: Record<string, unknown>;
    svg: string;
  }) => request<Visualization>("POST", "/api/visualizations", payload),
};
