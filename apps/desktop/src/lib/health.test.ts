import { describe, expect, it } from "vitest";
import { deriveHealth, toneOf, ALL_MODEL_STATUSES } from "./health";
import type { ModelEntry } from "../bindings/ModelEntry";

const entry = (category: ModelEntry["category"], status: ModelEntry["status"]): ModelEntry => ({
  model_id: category, category, display_name: category, source_label: null, status, tone: toneOf(status, category),
  required: category === "embedding", runtime: null, dimension: null, memory_mb: null, validated_at: null,
  ttl: "minutes10", error_code: null, error_key: null,
});

// Same expectations as crates/core/src/model.rs and health.rs tests.
describe("status tones mirror Rust", () => {
  it("AVAILABLE is never green; loaded states are", () => {
    expect(toneOf("AVAILABLE", "llm")).toBe("amber");
    for (const s of ["READY", "BUSY", "IDLE"] as const) expect(toneOf(s, "llm")).toBe("green");
  });
  it("missing required is red, missing optional is neutral", () => {
    expect(toneOf("MISSING", "embedding")).toBe("red");
    expect(toneOf("MISSING", "tts")).toBe("neutral");
  });
  it("errors are red", () => {
    expect(toneOf("ERROR", "stt")).toBe("red");
    expect(toneOf("INCOMPATIBLE", "tts")).toBe("red");
  });
  it("every status has a tone", () => {
    for (const s of ALL_MODEL_STATUSES) expect(["green", "amber", "red", "neutral"]).toContain(toneOf(s, "llm"));
  });
});

describe("deriveHealth", () => {
  it("no models → blocked", () => {
    const h = deriveHealth([], 0);
    expect(h.level).toBe("blocked");
    expect(h.models).toHaveLength(4);
  });
  it("validated embedding on disk + folder → ready (loads on demand)", () => {
    const h = deriveHealth([entry("embedding", "AVAILABLE")], 2);
    expect(h.level).toBe("ready");
    expect(h.semantic_search_enabled).toBe(true);
  });
  it("registered but not validated → degraded, search off", () => {
    const h = deriveHealth([entry("embedding", "REGISTERED")], 2);
    expect(h.level).toBe("degraded");
    expect(h.semantic_search_enabled).toBe(false);
  });
  it("embedding loaded + folder → ready; optional models do not matter", () => {
    const h = deriveHealth([entry("embedding", "READY"), entry("tts", "INCOMPATIBLE")], 1);
    expect(h.level).toBe("ready");
  });
  it("embedding loaded without folders → degraded but searchable", () => {
    const h = deriveHealth([entry("embedding", "IDLE")], 0);
    expect(h.level).toBe("degraded");
    expect(h.semantic_search_enabled).toBe(true);
  });
});
