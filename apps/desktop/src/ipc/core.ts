// The real Rust core: health, models (P4), folders (P1), search (P2) and
// history (P3). In a plain browser (no core) lists are empty and actions
// answer NOT_IMPLEMENTED; the UI renders that state honestly.
import { Channel } from "@tauri-apps/api/core";
import type { Api } from "./api";
import { call, isTauri, on } from "./bridge";
import type { AppHealth } from "../bindings/AppHealth";
import type { IpcError } from "../bindings/IpcError";
import type { ModelEntry } from "../bindings/ModelEntry";
import type { HistorySession } from "../bindings/HistorySession";
import type { HistoryTurn } from "../bindings/HistoryTurn";
import type { IndexStats } from "../bindings/IndexStats";
import type { RegisterModelRequest } from "../bindings/RegisterModelRequest";
import type { ResourceUsage } from "../bindings/ResourceUsage";
import type { ResultPreview } from "../bindings/ResultPreview";
import type { RevokeRequest } from "../bindings/RevokeRequest";
import type { ScopePreview } from "../bindings/ScopePreview";
import type { ScopeSummary } from "../bindings/ScopeSummary";
import type { SearchRequest } from "../bindings/SearchRequest";
import type { SearchResponse } from "../bindings/SearchResponse";
import type { TtlPolicy } from "../bindings/TtlPolicy";
import type { ValidationStep } from "../bindings/ValidationStep";

const noCore = (command: string): IpcError => ({
  code: "NOT_IMPLEMENTED",
  detail: `${command} (no desktop core in browser)`,
});


export class CoreApi implements Api {
  private listeners = new Set<() => void>();
  private unlisten: Array<() => void> = [];

  constructor() {
    const notify = () => this.listeners.forEach((l) => l());
    for (const ev of ["tray_action", "model_status", "scope_status"]) {
      on(ev, notify).then((u) => this.unlisten.push(u));
    }
  }
  dispose() { this.unlisten.forEach((u) => u()); }

  subscribe(cb: () => void) {
    this.listeners.add(cb);
    return () => { this.listeners.delete(cb); };
  }

  async health(): Promise<AppHealth> {
    if (!isTauri()) {
      // Browser without the core: nothing is installed.
      const { deriveHealth } = await import("../lib/health");
      return { ...deriveHealth([], 0), db_ok: false, level: "blocked" };
    }
    return call<AppHealth>("get_app_health");
  }

  async listModels(): Promise<ModelEntry[]> {
    if (!isTauri()) return [];
    return call<ModelEntry[]>("list_models");
  }

  async loadModel(modelId: string) {
    if (!isTauri()) throw noCore("load_model");
    await call("load_model", { modelId });
    this.listeners.forEach((l) => l());
  }

  async unloadModel(modelId: string) {
    if (!isTauri()) throw noCore("unload_model");
    await call("unload_model", { modelId });
  }

  async unloadAll() {
    if (!isTauri()) throw noCore("unload_all_models");
    await call("unload_all_models");
  }

  async setTtl(modelId: string, ttl: TtlPolicy) {
    if (!isTauri()) throw noCore("set_model_ttl");
    await call("set_model_ttl", { modelId, ttl });
    this.listeners.forEach((l) => l());
  }

  async removeModel(modelId: string) {
    if (!isTauri()) throw noCore("remove_model");
    await call("remove_model", { modelId });
    this.listeners.forEach((l) => l());
  }

  async validateModel(req: RegisterModelRequest, onStep: (s: ValidationStep[]) => void, signal: AbortSignal) {
    if (!isTauri()) throw noCore("validate_model");
    const ch = new Channel<ValidationStep[]>();
    ch.onmessage = (steps) => { if (!signal.aborted) onStep(steps); };
    const abort = () => { call("cancel_validation").catch(() => {}); };
    signal.addEventListener("abort", abort, { once: true });
    try {
      return await call<boolean>("validate_model", { req, onStep: ch });
    } finally {
      signal.removeEventListener("abort", abort);
    }
  }

  async registerModel(req: RegisterModelRequest) {
    if (!isTauri()) throw noCore("register_model");
    const m = await call<ModelEntry>("register_model", { req });
    this.listeners.forEach((l) => l());
    return m;
  }

  private changed() { this.listeners.forEach((l) => l()); }

  async listScopes(): Promise<ScopeSummary[]> {
    if (!isTauri()) return [];
    return call<ScopeSummary[]>("list_scopes");
  }

  async indexStats(): Promise<IndexStats> {
    if (!isTauri()) return { scope_count: 0, file_count: null, index_mb: null };
    return call<IndexStats>("index_stats");
  }

  async previewScope(path: string): Promise<ScopePreview> {
    if (!isTauri()) throw noCore("preview_scope");
    return call<ScopePreview>("preview_scope", { path });
  }

  async grantScope(token: string): Promise<ScopeSummary> {
    if (!isTauri()) throw noCore("grant_scope");
    const s = await call<ScopeSummary>("grant_scope", { token });
    this.changed();
    return s;
  }

  async setWatching(scopeId: string, on: boolean) {
    if (!isTauri()) throw noCore("set_watching");
    await call("set_watching", { scopeId, on });
  }

  async startIndex(scopeId: string) {
    if (!isTauri()) throw noCore("start_index");
    await call("start_index", { scopeId });
    this.changed();
  }

  async cancelIndex(scopeId: string) {
    if (!isTauri()) throw noCore("cancel_index");
    await call("cancel_index", { scopeId });
  }

  async revokeScope(req: RevokeRequest) {
    if (!isTauri()) throw noCore("revoke_scope");
    await call("revoke_scope", { req });
    this.changed();
  }

  async relinkScope(scopeId: string, path: string) {
    if (!isTauri()) throw noCore("relink_scope");
    await call("relink_scope", { scopeId, path });
  }

  async search(req: SearchRequest): Promise<SearchResponse> {
    if (!isTauri()) throw noCore("search");
    const r = await call<SearchResponse>("search", { req });
    this.changed();
    return r;
  }

  async preview(resultRef: string): Promise<ResultPreview> {
    if (!isTauri()) throw noCore("preview_result");
    return call<ResultPreview>("preview_result", { resultRef });
  }

  async openResult(resultRef: string, reveal: boolean) {
    if (!isTauri()) throw noCore("open_result");
    await call("open_result", { resultRef, reveal });
    this.changed();
  }

  async listHistory(): Promise<HistorySession[]> {
    if (!isTauri()) return [];
    return call<HistorySession[]>("list_history");
  }

  async sessionTurns(sessionId: string): Promise<HistoryTurn[]> {
    if (!isTauri()) return [];
    return call<HistoryTurn[]>("session_turns", { sessionId });
  }

  async deleteSession(sessionId: string) {
    if (!isTauri()) throw noCore("delete_session");
    await call("delete_session", { sessionId });
    this.changed();
  }

  async clearHistory() {
    if (!isTauri()) throw noCore("clear_history");
    await call("clear_history");
    this.changed();
  }

  async resourceUsage(): Promise<ResourceUsage> {
    if (!isTauri()) return { models_ram_mb: null, running_models: 0, gpu: null };
    return call<ResourceUsage>("resource_usage");
  }

  async exportDiagnostics(path: string) {
    if (!isTauri()) throw noCore("export_diagnostics");
    await call("export_diagnostics", { path });
  }

  async openDataFolder() {
    if (!isTauri()) throw noCore("open_data_folder");
    await call("open_data_folder");
  }
}
