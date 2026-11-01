import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import en from "./en.json";
import tr from "./tr.json";
import { translate } from "./index";
import { ALL_MODEL_STATUSES, CATEGORIES } from "../lib/health";

const enKeys = new Set(Object.keys(en));
const has = (k: string) => enKeys.has(k) || (enKeys.has(`${k}.one`) && enKeys.has(`${k}.other`));

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((f: string): string[] => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === "bindings" ? [] : sourceFiles(p);
    return /\.(tsx?|json)$/.test(f) && !/\.test\./.test(f) && !f.endsWith(".json") ? [p] : [];
  });
}

describe("dictionaries", () => {
  it("TR and EN have exactly the same keys", () => {
    expect(Object.keys(tr).sort()).toEqual(Object.keys(en).sort());
  });

  it("no empty translations", () => {
    for (const [k, v] of Object.entries(tr)) expect(v, k).not.toBe("");
  });

  it("every literal key used in the source exists", () => {
    const ns = "common|nav|window|privacy|mock|health|category|status|legend|panel|search|result|file|match|preview|voice|memory|models|model|ttl|addModel|folders|revoke|settings|welcome|widget|error";
    const re = new RegExp(`"((?:${ns})\\.[A-Za-z0-9_.]+)"`, "g");
    const missing: string[] = [];
    for (const f of sourceFiles(join(dirname(fileURLToPath(import.meta.url)), ".."))) {
      for (const m of readFileSync(f, "utf8").matchAll(re)) {
        const k = m[1];
        if (k.endsWith(".")) continue;
        if (!has(k)) missing.push(`${k} (${f.split("src")[1]})`);
      }
    }
    expect(missing).toEqual([]);
  });

  it("dynamic key families are complete", () => {
    const codes = ["MODEL_MISSING", "MODEL_INCOMPATIBLE", "RUNTIME_LOAD_FAILED", "INFERENCE_TIMEOUT", "EMBEDDING_GENERATION_STALE", "SCOPE_NOT_AUTHORIZED", "SCOPE_REVOKED", "SOURCE_CHANGED", "INDEX_UNAVAILABLE", "MIC_PERMISSION_DENIED", "DB_MIGRATION_FAILED", "USER_CANCELLED", "HOTKEY_CONFLICT", "NOT_IMPLEMENTED", "INTERNAL"];
    const keys = [
      ...codes.flatMap((c) => [`error.${c}.title`, `error.${c}.body`]),
      ...ALL_MODEL_STATUSES.map((s) => `status.${s}`), "status.OPTIONAL",
      ...CATEGORIES.map((c) => `category.${c}`),
      ...CATEGORIES.map((c) => `addModel.step.format.${c}`),
      ...CATEGORIES.map((c) => `addModel.supported.${c}`),
      ...CATEGORIES.map((c) => `addModel.fail.readable.${c}`),
      "addModel.fail.format.llm", "addModel.fail.format.tts",
      ...["up_to_date", "scanning", "paused", "unreachable", "revoking"].map((s) => `folders.status.${s}`),
      ...["ready", "degraded", "blocked"].map((s) => `health.level.${s}`),
      ...["green", "amber", "red", "neutral"].map((s) => `legend.${s}`),
      ...["previous_result", "semantic", "ocr"].map((s) => `match.${s}`),
      ...["today", "yesterday", "week", "older"].map((s) => `memory.bucket.${s}`),
      ...["llm", "stt", "tts"].map((s) => `models.desc.${s}`),
      ...["search", "memory", "models", "folders", "settings"].map((s) => `nav.${s}`),
      ...["minutes5", "minutes10", "minutes30", "hours1", "never"].map((s) => `ttl.${s}`),
      ...["markdown", "text", "pdf", "image", "other"].map((s) => `file.${s}`),
    ];
    expect(keys.filter((k) => !has(k))).toEqual([]);
  });
});

describe("translate", () => {
  it("English is the fallback and params are filled", () => {
    expect(translate("tr", "nav.search")).toBe("Ara");
    expect(translate("en", "nav.search")).toBe("Search");
    expect(translate("tr", "no.such.key")).toBe("no.such.key");
    expect(translate("en", "folders.grantedToast", { path: "C:/x" })).toBe("C:/x added. Scanning started.");
  });

  it("plurals pick .one / .other", () => {
    expect(translate("en", "memory.results", { n: 1 })).toBe("1 result");
    expect(translate("en", "memory.results", { n: 3 })).toBe("3 results");
    expect(translate("tr", "memory.results", { n: 3 })).toBe("3 sonuç");
  });
});
