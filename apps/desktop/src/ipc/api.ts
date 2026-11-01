// The contract the UI talks to. Two implementations:
//  - mock.ts: labelled MOCK ONLY demo data until backend phases P1–P4 land;
//  - real.ts: the Rust core (health/settings now, the rest NOT_IMPLEMENTED).
import type { AppHealth } from "../bindings/AppHealth";
import type { HistorySession } from "../bindings/HistorySession";
import type { HistoryTurn } from "../bindings/HistoryTurn";
import type { IndexStats } from "../bindings/IndexStats";
import type { ModelEntry } from "../bindings/ModelEntry";
import type { RegisterModelRequest } from "../bindings/RegisterModelRequest";
import type { ResultPreview } from "../bindings/ResultPreview";
import type { RevokeRequest } from "../bindings/RevokeRequest";
import type { ScopePreview } from "../bindings/ScopePreview";
import type { ScopeSummary } from "../bindings/ScopeSummary";
import type { SearchRequest } from "../bindings/SearchRequest";
import type { SearchResponse } from "../bindings/SearchResponse";
import type { TtlPolicy } from "../bindings/TtlPolicy";
import type { ValidationStep } from "../bindings/ValidationStep";

export interface Api {
  readonly mode: "mock" | "real";
  /** Notifies when data changed (other window, tray action, timers). */
  subscribe(cb: () => void): () => void;

  health(): Promise<AppHealth>;

  listModels(): Promise<ModelEntry[]>;
  loadModel(id: string): Promise<void>;
  unloadModel(id: string): Promise<void>;
  unloadAll(): Promise<void>;
  setTtl(id: string, ttl: TtlPolicy): Promise<void>;
  removeModel(id: string): Promise<void>;
  /** Streams validation steps; resolves true when every step passed. */
  validateModel(req: RegisterModelRequest, onStep: (s: ValidationStep[]) => void, signal: AbortSignal): Promise<boolean>;
  registerModel(req: RegisterModelRequest): Promise<ModelEntry>;

  listScopes(): Promise<ScopeSummary[]>;
  indexStats(): Promise<IndexStats>;
  previewScope(path: string): Promise<ScopePreview>;
  grantScope(token: string): Promise<ScopeSummary>;
  setWatching(id: string, on: boolean): Promise<void>;
  startIndex(id: string): Promise<void>;
  cancelIndex(id: string): Promise<void>;
  revokeScope(req: RevokeRequest): Promise<void>;
  relinkScope(id: string): Promise<void>;

  search(req: SearchRequest): Promise<SearchResponse>;
  preview(resultRef: string): Promise<ResultPreview>;
  openResult(resultRef: string, reveal: boolean): Promise<void>;

  listHistory(): Promise<HistorySession[]>;
  sessionTurns(sessionId: string): Promise<HistoryTurn[]>;
  deleteSession(sessionId: string): Promise<void>;
  clearHistory(): Promise<void>;
}

// The conversation shown in Search, shared by both windows (widget → app).
const SESSION_KEY = "tamgabul.currentSession";
export const currentSession = {
  get(): string | null {
    try { return localStorage.getItem(SESSION_KEY); } catch { return null; }
  },
  set(id: string | null) {
    try {
      if (id) localStorage.setItem(SESSION_KEY, id);
      else localStorage.removeItem(SESSION_KEY);
    } catch { /* storage unavailable: per-window only */ }
  },
};
