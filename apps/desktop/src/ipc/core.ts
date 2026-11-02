// The real Rust core. Health is live today; the rest of the contract answers
// NOT_IMPLEMENTED until its backend phase lands (P1 scopes, P2 search,
// P3 history, P4 models). The UI renders that state honestly.
import type { Api } from "./api";
import { call, isTauri, on } from "./bridge";
import type { AppHealth } from "../bindings/AppHealth";
import type { IpcError } from "../bindings/IpcError";
import type { ModelEntry } from "../bindings/ModelEntry";

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
    on("tray_action", () => this.listeners.forEach((l) => l())).then((u) => this.unlisten.push(u));
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

  /** Until P4 the registry has no runtime data; show the four slots from health. */
  async listModels(): Promise<ModelEntry[]> {
    const h = await this.health();
    return h.models
      .filter((m) => m.status !== "MISSING")
      .map((m) => ({
        model_id: `db-${m.category}`, category: m.category, display_name: m.display_name ?? m.category,
        source_label: null, status: m.status, tone: m.tone, required: m.required, runtime: null,
        dimension: null, memory_mb: null, validated_at: null, ttl: "minutes10", error_code: null, error_key: null,
      }));
  }

  loadModel = () => notYet("load_model");
  unloadModel = () => notYet("unload_model");
  unloadAll = () => notYet("unload_model");
  setTtl = () => notYet("set_model_policy");
  removeModel = () => notYet("remove_model");
  validateModel = () => notYet("validate_model");
  registerModel = () => notYet("register_model");

  listScopes = () => notYet("list_scopes");
  indexStats = () => notYet("list_scopes");
  previewScope = () => notYet("preview_scope");
  grantScope = () => notYet("grant_scope");
  setWatching = () => notYet("pause_scope");
  startIndex = () => notYet("start_index_job");
  cancelIndex = () => notYet("cancel_job");
  revokeScope = () => notYet("revoke_scope");
  relinkScope = () => notYet("relink_scope");

  search = () => notYet("submit_utterance");
  preview = () => notYet("preview_result");
  openResult = () => notYet("open_result");

  listHistory = () => notYet("list_history");
  sessionTurns = () => notYet("list_history");
  deleteSession = () => notYet("delete_history");
  clearHistory = () => notYet("delete_history");
}
