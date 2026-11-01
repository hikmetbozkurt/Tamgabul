// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { MockApi } from "./mock";
import type { ValidationStep } from "../bindings/ValidationStep";

beforeEach(() => localStorage.clear());

describe("MockApi (MOCK ONLY demo backend)", () => {
  it("first run has no models and refuses to search", async () => {
    const api = new MockApi("first_run");
    expect(await api.listModels()).toEqual([]);
    expect((await api.health()).level).toBe("blocked");
    await expect(api.search({ session_id: null, text: "timer", choice_id: null })).rejects.toMatchObject({ code: "MODEL_MISSING" });
    api.dispose();
  });

  it("vague query → clarification → choice narrows to one project", async () => {
    const api = new MockApi("ready");
    const a = await api.search({ session_id: null, text: "the button on the game screen", choice_id: null });
    expect(a.kind).toBe("clarify");
    expect(a.choices.map((c) => c.label)).toContain("20coin Clash");
    const b = await api.search({ session_id: a.session_id, text: "20coin Clash", choice_id: "proj:20coin" });
    expect(b.kind).toBe("results");
    expect(b.results.every((r) => r.project === "20coin Clash")).toBe(true);
    const turns = await api.sessionTurns(a.session_id);
    expect(turns.map((t) => t.role)).toEqual(["user", "assistant", "user", "assistant"]);
    api.dispose();
  });

  it("unknown topic → not_found, never an invented result", async () => {
    const api = new MockApi("ready");
    const r = await api.search({ session_id: null, text: "grandma's pilaf recipe", choice_id: null });
    expect(r.kind).toBe("not_found");
    expect(r.results).toEqual([]);
    api.dispose();
  });

  it("LLM not loaded marks responses as degraded", async () => {
    const api = new MockApi("llm_off");
    const r = await api.search({ session_id: null, text: "auction timer", choice_id: null });
    expect(r.degraded_llm).toBe(true);
    api.dispose();
  });

  it("revoking a folder removes its results from history at once", async () => {
    const api = new MockApi("ready");
    await api.revokeScope({ scope_id: "s-notes", mode: "remove_and_purge", purge_history: false });
    const all = await api.listHistory();
    for (const s of all) {
      for (const t of await api.sessionTurns(s.session_id)) {
        expect(t.results.some((r) => r.display_path.startsWith("D:/Notes"))).toBe(false);
      }
    }
    expect((await api.listScopes()).some((s) => s.scope_id === "s-notes")).toBe(false);
    api.dispose();
  });

  it("stop watching keeps the folder (no purge)", async () => {
    const api = new MockApi("ready");
    await api.revokeScope({ scope_id: "s-projects", mode: "stop_watching", purge_history: false });
    const s = (await api.listScopes()).find((x) => x.scope_id === "s-projects");
    expect(s).toMatchObject({ watching: false, status: "paused" });
    api.dispose();
  });

  it("drive root preview is flagged and excludes system folders", async () => {
    const api = new MockApi("ready");
    const p = await api.previewScope("C:\\");
    expect(p.is_drive_root).toBe(true);
    expect(p.auto_excludes).toContain("Windows");
    api.dispose();
  });

  it("validation fails a non-GGUF LLM and registers a good one as AVAILABLE (not green)", async () => {
    const api = new MockApi("first_run");
    let last: ValidationStep[] = [];
    const bad = await api.validateModel({ category: "llm", path: "C:/m/model.bin" }, (s) => (last = s), new AbortController().signal);
    expect(bad).toBe(false);
    expect(last.find((s) => s.state === "failed")?.id).toBe("format");
    const ok = await api.validateModel({ category: "embedding", path: "C:/m/embeddinggemma-2" }, () => {}, new AbortController().signal);
    expect(ok).toBe(true);
    const m = await api.registerModel({ category: "embedding", path: "C:/m/embeddinggemma-2" });
    expect(m.status).toBe("AVAILABLE");
    expect(m.tone).toBe("amber");
    expect((await api.health()).semantic_search_enabled).toBe(false);
    api.dispose();
  });

  it("validation can be cancelled", async () => {
    const api = new MockApi("ready");
    const ctl = new AbortController();
    const p = api.validateModel({ category: "llm", path: "x.gguf" }, () => {}, ctl.signal);
    ctl.abort();
    await expect(p).rejects.toMatchObject({ code: "USER_CANCELLED" });
    api.dispose();
  });

  it("state is shared through localStorage (widget ↔ app)", async () => {
    const a = new MockApi("ready");
    const r = await a.search({ session_id: null, text: "auction timer", choice_id: null });
    const b = new MockApi("ready");
    expect((await b.listHistory()).some((s) => s.session_id === r.session_id)).toBe(true);
    a.dispose();
    b.dispose();
  });
});
