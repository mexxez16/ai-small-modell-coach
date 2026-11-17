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
  dataset_id?: string | null;
  dataset_hash?: string | null;
  model?: string | null;
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

export type Scope = {
  id: string;
  name: string;
  query: string;
  created_at: string;
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

export type ModelEntry = {
  id: string;
  name: string;
  path: string;
  kind: "shipped" | "local" | "finetuned";
  run_id?: string | null;
  notes: string;
  created_at: string;
};

export type SourceDoc = {
  id: string;
  name: string;
  kind: string;
  size: number;
  sha256: string;
  content: string;
  created_at: string;
};

export type ChunkResponse = {
  dataset_id: string;
  created_items: number;
  skipped_existing: number;
};

export type AnnotateResponse = {
  annotated: number;
  failed: number;
  errors: string[];
};

export type DataItem = {
  id: string;
  dataset_id: string;
  text?: string | null;
  subject?: string | null;
  predicate?: string | null;
  object?: string | null;
  label: string;
  kind: string;
  source: string;
  status: string;
  split: string;
  assignee?: string | null;
  notes?: string | null;
  updated_at: string;
};

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

export type DataSession = {
  id: string;
  name: string;
  notes: string;
  created_at: string;
  updated_at: string;
};

export type DataRunStage = {
  kind: "structured_import" | "ssot_import" | "chunk" | "ai_annotate" | "export";
  config: Record<string, unknown>;
};

export type DataRun = {
  id: string;
  session_id: string | null;
  name: string;
  status: "draft" | "completed" | "failed" | "running";
  stages: DataRunStage[];
  dataset_id: string | null;
  results: Record<string, unknown>;
  created_at: string;
  updated_at: string;
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
  createRun: (name: string, sessionId?: string, _tags?: string[], model?: string, datasetId?: string) =>
    request<Run>("POST", "/api/runs", {
      name,
      session_id: sessionId,
      hparams: {},
      ...(model ? { model } : {}),
      ...(datasetId ? { dataset_id: datasetId } : {}),
    }),
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
  getVibeSettings: () =>
    request<{ config_path: string; config_exists: boolean; active_model: string | null }>(
      "GET",
      "/api/settings/vibe",
    ),
  setVibeModel: (activeModel: string) =>
    request<{ config_path: string; config_exists: boolean; active_model: string }>(
      "PUT",
      "/api/settings/vibe",
      { active_model: activeModel },
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
  listScopes: () => request<Scope[]>("GET", "/api/scopes"),
  saveScope: (name: string, query: string) =>
    request<Scope>("POST", "/api/scopes", { name, query }),
  deleteScope: (id: string) => request<void>("DELETE", `/api/scopes/${id}`),
  saveVisualization: (payload: {
    run_id?: string;
    worker_model: string;
    text?: string;
    style: string;
    options?: Record<string, unknown>;
    svg: string;
  }) => request<Visualization>("POST", "/api/visualizations", payload),
  listModels: () => request<ModelEntry[]>("GET", "/api/models"),
  updateDatasetTags: (id: string, tags: string[]) =>
    request<{ id: string; tags: string[] }>("POST", `/api/datasets/${id}/tags`, { tags }),
  createModel: (name: string, path: string, kind?: string) =>
    request<ModelEntry>("POST", "/api/models", { name, path, kind }),
  deleteModel: (id: string) => request<void>("DELETE", `/api/models/${id}`),
  listSources: () => request<SourceDoc[]>("GET", "/api/sources"),  createSource: (name: string, content: string) =>
    request<{ id: string; name: string }>("POST", "/api/sources", { name, content }),
  deleteSource: (id: string) => request<void>("DELETE", `/api/sources/${id}`),
  chunkSource: (
    sourceId: string,
    payload: {
      strategy: string;
      max_chars?: number;
      overlap?: number;
      dataset_id?: string;
      label?: string;
    },
  ) => request<ChunkResponse>("POST", `/api/sources/${sourceId}/chunks`, payload),
  annotate: (payload: {
    provider_id: string;
    item_ids: string[];
    instruction?: string;
    labels?: string[];
  }) => request<AnnotateResponse>("POST", "/api/annotations", payload),
  // --- Training-data sessions + runs (curation pipeline) ------------------
  listDataSessions: () => request<DataSession[]>("GET", "/api/data-sessions"),
  createDataSession: (name: string, notes = "") =>
    request<DataSession>("POST", "/api/data-sessions", { name, notes }),
  deleteDataSession: (id: string) =>
    request<void>("DELETE", `/api/data-sessions/${id}`),
  listDataSessionRuns: (id: string) =>
    request<DataRun[]>("GET", `/api/data-sessions/${id}/runs`),
  listDataRuns: (sessionId?: string) =>
    request<DataRun[]>(
      "GET",
      sessionId
        ? `/api/data-runs?session_id=${encodeURIComponent(sessionId)}`
        : "/api/data-runs",
    ),
  getDataRun: (id: string) => request<DataRun>("GET", `/api/data-runs/${id}`),
  createDataRun: (
    name: string,
    sessionId?: string,
    stages: DataRunStage[] = [],
    datasetId?: string,
  ) =>
    request<DataRun>("POST", "/api/data-runs", {
      name,
      ...(sessionId ? { session_id: sessionId } : {}),
      stages,
      ...(datasetId ? { dataset_id: datasetId } : {}),
    }),
  updateDataRun: (
    id: string,
    patch: { name?: string; session_id?: string; stages?: DataRunStage[] },
  ) => request<DataRun>("PUT", `/api/data-runs/${id}`, patch),
  deleteDataRun: (id: string) => request<void>("DELETE", `/api/data-runs/${id}`),
  executeDataRun: (id: string) => request<DataRun>("POST", `/api/data-runs/${id}/execute`),
  getStepConfig: <T = Record<string, unknown>>(step: string) =>
    request<T>("GET", `/api/config/${step}`),
  putStepConfig: (step: string, value: unknown) =>
    request<unknown>("PUT", `/api/config/${step}`, value),
  // Assistent (goal advisor)
  assistantStatus: () =>
    request<{
      goal: unknown | null;
      config_steps: Record<string, { present: boolean; bytes: number }>;
      providers_openai_compatible: number;
      docs_mirror_versions: string[];
    }>("GET", "/api/assistant/status"),
  assistantSpacyStatus: (python?: string) =>
    request<{
      python: string;
      installed: boolean;
      spacy_version?: string | null;
      installed_models?: string[];
      error?: string | null;
    }>(
      "GET",
      `/api/assistant/spacy-status${python ? `?python=${encodeURIComponent(python)}` : ""}`,
    ),
  assistantDataProfile: () =>
    request<{
      datasets: Array<{
        dataset_id: string;
        name: string;
        source_ref: string;
        items: number;
        by_status: Record<string, number>;
        by_kind: Record<string, number>;
        by_label: Record<string, number>;
        items_without_text: number;
        duplicate_texts: number;
      }>;
    }>("GET", "/api/assistant/data-profile"),
  assistantTurn: (
    providerId: string,
    messages: Array<{ role: "user" | "assistant"; content: string }>,
  ) =>
    request<{
      reply: string;
      tool_trace: Array<{ round: number; tool: string; query?: string; ok: boolean }>;
      proposals: AssistantProposal[];
    }>("POST", "/api/assistant/turn", { provider_id: providerId, messages }),
  assistantProposals: () =>
    request<{ proposals: AssistantProposal[] }>("GET", "/api/assistant/proposals"),
  assistantAcceptProposal: (id: string) =>
    request<{ accepted: string }>("POST", "/api/assistant/proposals/accept", { id }),
  assistantRejectProposal: (id: string) =>
    request<{ rejected: string }>("POST", "/api/assistant/proposals/reject", { id }),
  assistantDocsPull: (version: string) =>
    request<{ version: string; path: string }>("POST", "/api/assistant/docs/pull", {
      version,
    }),
  assistantDocsSearch: (q: string, version?: string) =>
    request<{ version: string; matches: Array<{ file: string; snippet: string }> }>(
      "GET",
      `/api/assistant/docs/search?q=${encodeURIComponent(q)}${
        version ? `&version=${encodeURIComponent(version)}` : ""
      }`,
    ),
};

export type AssistantProposal = {
  id: string;
  created_at: string;
  kind: "step_config" | "goal";
  step?: string;
  summary: string;
  payload: {
    tool: string;
    step?: string;
    config?: unknown;
    goal?: unknown;
  };
};
