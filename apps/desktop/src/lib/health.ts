// Mirrors crates/core model.rs + health.rs so demo data obeys the same rules.
// Tests in lib/health.test.ts keep the two in step.
import type { AppHealth } from "../bindings/AppHealth";
import type { ModelCategory } from "../bindings/ModelCategory";
import type { ModelEntry } from "../bindings/ModelEntry";
import type { ModelStatus } from "../bindings/ModelStatus";
import type { StatusTone } from "../bindings/StatusTone";

export const ALL_MODEL_STATUSES = [
  "MISSING", "REGISTERED", "VALIDATING", "AVAILABLE", "LOADING", "READY",
  "BUSY", "IDLE", "UNLOADING", "ERROR", "INCOMPATIBLE",
] as const satisfies readonly ModelStatus[];

type Unlisted = Exclude<ModelStatus, (typeof ALL_MODEL_STATUSES)[number]>;
// Compile error here = Rust added a status that is not listed above.
export const _exhaustive: Unlisted extends never ? true : never = true;

export const CATEGORIES: ModelCategory[] = ["embedding", "llm", "stt", "tts"];

export const isLoaded = (s: ModelStatus) => s === "READY" || s === "BUSY" || s === "IDLE";

export function toneOf(status: ModelStatus, category: ModelCategory): StatusTone {
  if (isLoaded(status)) return "green";
  switch (status) {
    case "REGISTERED": case "VALIDATING": case "AVAILABLE": case "LOADING": case "UNLOADING":
      return "amber";
    case "ERROR": case "INCOMPATIBLE":
      return "red";
    case "MISSING":
      return category === "embedding" ? "red" : "neutral";
    default:
      return "neutral";
  }
}

export function deriveHealth(models: ModelEntry[], activeScopes: number): AppHealth {
  const byCat = (c: ModelCategory) => models.find((m) => m.category === c);
  const summaries = CATEGORIES.map((c) => {
    const m = byCat(c);
    const status: ModelStatus = m?.status ?? "MISSING";
    return {
      category: c,
      status,
      tone: toneOf(status, c),
      required: c === "embedding",
      display_name: m?.display_name ?? null,
    };
  });
  const emb = summaries[0];
  const semantic = isLoaded(emb.status);
  const level = emb.tone === "red" ? "blocked" : semantic && activeScopes > 0 ? "ready" : "degraded";
  return {
    api_version: 1,
    level,
    db_ok: true,
    semantic_search_enabled: semantic,
    active_scope_count: activeScopes,
    models: summaries,
  };
}
