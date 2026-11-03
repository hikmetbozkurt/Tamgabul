// The real Rust core. Health, models (P4) and folders (P1) are live; search
// (P2) and history (P3) answer NOT_IMPLEMENTED until their phase lands. The
// UI renders that state honestly.
import { Channel } from "@tauri-apps/api/core";
import type { Api } from "./api";
import { call, isTauri, on } from "./bridge";
import type { AppHealth } from "../bindings/AppHealth";
import type { IpcError } from "../bindings/IpcError";
import type { ModelEntry } from "../bindings/ModelEntry";
import type { IndexStats } from "../bindings/IndexStats";
import type { RegisterModelRequest } from "../bindings/RegisterModelRequest";
import type { RevokeRequest } from "../bindings/RevokeRequest";
import type { ScopePreview } from "../bindings/ScopePreview";
import type { ScopeSummary } from "../bindings/ScopeSummary";
import type { TtlPolicy } from "../bindings/TtlPolicy";
import type { ValidationStep } from "../bindings/ValidationStep";

const noCore = (command: string): IpcError => ({
  code: "NOT_IMPLEMENTED",
  detail: `${command} (no desktop core in browser)`,
});

const notYet = async (command: string): Promise<never> => {
  if (!isTauri()) {
    throw { code: "NOT_IMPLEMENTED", detail: `${command} (no desktop core in browser)` } satisfies IpcError;
  }
  await call("not_implemented", { command });
  throw { code: "NOT_IMPLEMENTED", detail: command } satisfies IpcError;
};

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

  async relinkScope(scopeId: string) {
    if (!isTauri()) throw noCore("relink_scope");
    await call("relink_scope", { scopeId });
  }

  search = () => notYet("submit_utterance");
  preview = () => notYet("preview_result");
  openResult = () => notYet("open_result");

  listHistory = () => notYet("list_history");
  sessionTurns = () => notYet("list_history");
  deleteSession = () => notYet("delete_history");
  clearHistory = () => notYet("delete_history");
}
