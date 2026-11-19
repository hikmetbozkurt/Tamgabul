// @vitest-environment jsdom
// P5 component and accessibility tests using a test-only FakeApi (never shipped, ADR 018).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { Root } from "./Root";
import { comboFromEvent } from "./features/settings/SettingsPage";
import { DEFAULT_SETTINGS } from "./state/settings";
import type { Api } from "./ipc/api";
import type { AppHealth } from "./bindings/AppHealth";
import type { AppSettings } from "./bindings/AppSettings";
import type { DownloadModelRequest } from "./bindings/DownloadModelRequest";
import type { DownloadProgress } from "./bindings/DownloadProgress";
import type { DownloadableModel } from "./bindings/DownloadableModel";
import type { HistorySession } from "./bindings/HistorySession";
import type { HistoryTurn } from "./bindings/HistoryTurn";
import type { IndexStats } from "./bindings/IndexStats";
import type { ModelEntry } from "./bindings/ModelEntry";
import type { RegisterModelRequest } from "./bindings/RegisterModelRequest";
import type { ResourceUsage } from "./bindings/ResourceUsage";
import type { ResultPreview } from "./bindings/ResultPreview";
import type { RevokeRequest } from "./bindings/RevokeRequest";
import type { ScopeExclusion } from "./bindings/ScopeExclusion";
import type { ScopePreview } from "./bindings/ScopePreview";
import type { ScopeSummary } from "./bindings/ScopeSummary";
import type { SearchRequest } from "./bindings/SearchRequest";
import type { SearchResponse } from "./bindings/SearchResponse";
import type { SettingsSnapshot } from "./bindings/SettingsSnapshot";
import type { SynthesizeRequest } from "./bindings/SynthesizeRequest";
import type { SynthesizeResponse } from "./bindings/SynthesizeResponse";
import type { TranscribeRequest } from "./bindings/TranscribeRequest";
import type { TranscribeResponse } from "./bindings/TranscribeResponse";
import type { TtlPolicy } from "./bindings/TtlPolicy";
import type { ValidationStep } from "./bindings/ValidationStep";
import * as audioLib from "./lib/audio";
import { deriveHealth } from "./lib/health";

class FakeApi implements Api {
  private listeners = new Set<() => void>();
  models: ModelEntry[] = [];
  scopes: ScopeSummary[] = [];
  stats: IndexStats = { scope_count: 0, file_count: 0, index_mb: 0 };
  usage: ResourceUsage = { models_ram_mb: null, running_models: 0, gpu: null };
  downloadable: DownloadableModel[] = [];
  exclusionsByScope: Record<string, ScopeExclusion[]> = {};
  previewImpl: (path: string) => Promise<ScopePreview> = async (path) => ({
    preview_token: `tok-${path}`,
    display_path: path,
    is_drive_root: path === "C:\\" || path === "/",
    is_network: path.startsWith("\\\\"),
    file_count: 42,
    est_index_mb: 1.5,
    cap_files: 200000,
    auto_excludes: [".git", "node_modules"],
    parent_display_path: null,
    complete: true,
  });
  validateImpl: (
    req: RegisterModelRequest,
    onStep: (s: ValidationStep[]) => void,
  ) => Promise<boolean> = async (_req, onStep) => {
    onStep([
      { id: "readable", label_key: "addModel.step.readable", state: "ok", detail: null },
      { id: "format", label_key: "addModel.step.format.embedding", state: "ok", detail: "768d · BF16 · text + image" },
    ]);
    return true;
  };
  downloadImpl: (
    req: DownloadModelRequest,
    onProgress: (p: DownloadProgress) => void,
  ) => Promise<string> = async (req, onProgress) => {
    onProgress({
      preset_id: req.preset_id,
      phase: "verifying",
      file_index: 1,
      file_count: 1,
      file_name: "model.gguf",
      bytes_done: 100,
      bytes_total: 100,
      destination_path: req.destination_dir,
      error_key: null,
    });
    return req.destination_dir;
  };
  searchImpl: (req: SearchRequest) => Promise<SearchResponse> = async (req) => ({
    session_id: req.session_id ?? "c-1",
    turn_id: "a-1",
    kind: "not_found",
    count: 0,
    choices: [],
    results: [],
    question: null,
    understood: null,
    answer: null,
    summary: null,
    picked_by_llm: false,
    picked_count: 0,
    notice: null,
    degraded_llm: true,
    searched_scope_count: this.scopes.length,
  });

  grantedTokens: string[] = [];
  revokedRequests: RevokeRequest[] = [];
  registeredRequests: RegisterModelRequest[] = [];
  downloadRequests: DownloadModelRequest[] = [];
  loadedIds: string[] = [];
  transcribedRequests: TranscribeRequest[] = [];
  synthesizedRequests: SynthesizeRequest[] = [];
  stopSpeechCalls = 0;
  transcribeImpl: (req: TranscribeRequest) => Promise<TranscribeResponse> = async () => ({
    text: "meeting budget notes",
  });
  synthesizeImpl: (req: SynthesizeRequest) => Promise<SynthesizeResponse> = async () => ({
    wav_base64: audioLib.bytesToBase64(audioLib.encodeWav16kMono(new Int16Array(160), 22_050)),
    sample_rate: 22_050,
  });
  turnsBySession: Record<string, HistoryTurn[]> = {};

  notify() {
    this.listeners.forEach((l) => l());
  }
  subscribe(cb: () => void) {
    this.listeners.add(cb);
    return () => { this.listeners.delete(cb); };
  }
  async health(): Promise<AppHealth> {
    const active = this.scopes.filter((s) => s.status !== "revoking").length;
    return deriveHealth(this.models, active);
  }
  async listModels() { return this.models; }
  async loadModel(id: string) { this.loadedIds.push(id); }
  async unloadModel() {}
  async unloadAll() {}
  async setTtl(id: string, ttl: TtlPolicy) {
    this.models = this.models.map((m) => (m.model_id === id ? { ...m, ttl } : m));
    this.notify();
  }
  async removeModel(id: string) {
    this.models = this.models.filter((m) => m.model_id !== id);
    this.notify();
  }
  async validateModel(req: RegisterModelRequest, onStep: (s: ValidationStep[]) => void, _signal: AbortSignal) {
    return this.validateImpl(req, onStep);
  }
  async registerModel(req: RegisterModelRequest): Promise<ModelEntry> {
    this.registeredRequests.push(req);
    const m: ModelEntry = {
      model_id: `m-${this.registeredRequests.length}`,
      category: req.category,
      display_name: req.category === "embedding" ? "EmbeddingGemma 2" : "Gemma 4 E2B-it",
      source_label: "GGUF",
      status: "AVAILABLE",
      tone: "amber",
      required: req.category === "embedding",
      runtime: "llama.cpp",
      dimension: req.category === "embedding" ? 768 : null,
      memory_mb: null,
      validated_at: "2026-11-19T10:00:00Z",
      ttl: "minutes5",
      error_code: null,
      error_key: null,
    };
    this.models = [...this.models.filter((x) => x.category !== req.category), m];
    this.notify();
    return m;
  }
  async listDownloadableModels() { return this.downloadable; }
  async downloadModel(req: DownloadModelRequest, onProgress: (p: DownloadProgress) => void, _signal: AbortSignal) {
    this.downloadRequests.push(req);
    return this.downloadImpl(req, onProgress);
  }
  async listScopes() { return this.scopes; }
  async indexStats() { return this.stats; }
  async previewScope(path: string) { return this.previewImpl(path); }
  async grantScope(token: string) {
    this.grantedTokens.push(token);
    const path = token.replace(/^tok-/, "");
    const s: ScopeSummary = {
      scope_id: `s-${this.grantedTokens.length}`,
      display_path: path,
      status: "up_to_date",
      watching: false,
      file_count: 42,
      last_scan_at: "2026-11-19T10:00:00Z",
      progress: null,
      include_label: null,
      exclusion_count: 0,
      is_drive_root: path === "C:\\" || path === "/",
      is_network: path.startsWith("\\\\"),
      parent_scope_id: null,
      error_code: null,
    };
    this.scopes = [...this.scopes, s];
    this.notify();
    return s;
  }
  async setWatching() {}
  async startIndex() {}
  async cancelIndex() {}
  async revokeScope(req: RevokeRequest) {
    this.revokedRequests.push(req);
    this.scopes = this.scopes.filter((s) => s.scope_id !== req.scope_id);
    this.notify();
  }
  async relinkScope() {}
  async listScopeExclusions(scopeId: string) {
    return this.exclusionsByScope[scopeId] ?? [];
  }
  async addScopeExclusion(scopeId: string, path: string): Promise<ScopeExclusion> {
    const scope = this.scopes.find((s) => s.scope_id === scopeId);
    const display = path.includes(":") || path.startsWith("/") || path.startsWith("\\\\")
      ? path
      : `${scope?.display_path ?? ""}\\${path}`;
    const entry: ScopeExclusion = {
      exclusion_id: `x-${(this.exclusionsByScope[scopeId]?.length ?? 0) + 1}`,
      scope_id: scopeId,
      display_path: display,
      reason: "user_exclusion",
      created_at: "2026-11-19T10:00:00Z",
    };
    this.exclusionsByScope[scopeId] = [...(this.exclusionsByScope[scopeId] ?? []), entry];
    this.scopes = this.scopes.map((s) =>
      s.scope_id === scopeId ? { ...s, exclusion_count: this.exclusionsByScope[scopeId].length } : s,
    );
    this.notify();
    return entry;
  }
  async removeScopeExclusion(scopeId: string, exclusionId: string) {
    this.exclusionsByScope[scopeId] = (this.exclusionsByScope[scopeId] ?? []).filter(
      (e) => e.exclusion_id !== exclusionId,
    );
    this.scopes = this.scopes.map((s) =>
      s.scope_id === scopeId ? { ...s, exclusion_count: this.exclusionsByScope[scopeId].length } : s,
    );
    this.notify();
  }
  async search(req: SearchRequest) {
    const r = await this.searchImpl(req);
    const list = this.turnsBySession[r.session_id] ?? [];
    list.push(
      {
        turn_id: `u-${list.length + 1}`,
        role: "user",
        at: "2026-11-19T10:00:00Z",
        text: req.text,
        kind: null,
        count: null,
        choices: [],
        results: [],
        opened: false,
        question: null,
        understood: null,
        answer: null,
        summary: null,
        picked_by_llm: false,
        picked_count: 0,
        notice: null,
      },
      {
        turn_id: r.turn_id,
        role: "assistant",
        at: "2026-11-19T10:00:01Z",
        text: null,
        kind: r.kind,
        count: r.count,
        choices: r.choices,
        results: r.results,
        opened: false,
        question: r.question,
        understood: r.understood,
        answer: r.answer,
        summary: r.summary,
        picked_by_llm: r.picked_by_llm,
        picked_count: r.picked_count,
        notice: r.notice,
      },
    );
    this.turnsBySession[r.session_id] = list;
    this.notify();
    return r;
  }
  async preview(ref: string): Promise<ResultPreview> {
    for (const turns of Object.values(this.turnsBySession)) {
      for (const turn of turns) {
        const hit = turn.results.find((r) => r.result_ref === ref);
        if (hit) return { result: hit, body: hit.snippet ?? "" };
      }
    }
    throw { code: "NOT_IMPLEMENTED", detail: "unused" };
  }
  async openResult() {}
  async listHistory(): Promise<HistorySession[]> { return []; }
  async sessionTurns(id: string): Promise<HistoryTurn[]> { return this.turnsBySession[id] ?? []; }
  async deleteSession() {}
  async clearHistory() {}
  async resourceUsage() { return this.usage; }
  async exportDiagnostics() {}
  async openDataFolder() {}
  async transcribeAudio(req: TranscribeRequest): Promise<TranscribeResponse> {
    this.transcribedRequests.push(req);
    return this.transcribeImpl(req);
  }
  async synthesizeSpeech(req: SynthesizeRequest): Promise<SynthesizeResponse> {
    this.synthesizedRequests.push(req);
    return this.synthesizeImpl(req);
  }
  async stopSpeech(): Promise<void> {
    this.stopSpeechCalls += 1;
  }
}

function makeModel(partial: Partial<ModelEntry> & Pick<ModelEntry, "model_id" | "category" | "status" | "tone">): ModelEntry {
  return {
    display_name: partial.category === "embedding" ? "EmbeddingGemma 2" : "Gemma 4 E2B-it",
    source_label: "GGUF",
    required: partial.category === "embedding",
    runtime: "llama.cpp b12024",
    dimension: partial.category === "embedding" ? 768 : null,
    memory_mb: null,
    validated_at: "2026-11-19T10:00:00Z",
    ttl: "minutes5",
    error_code: null,
    error_key: null,
    ...partial,
  };
}

function makeScope(partial: Partial<ScopeSummary> & Pick<ScopeSummary, "scope_id" | "display_path">): ScopeSummary {
  return {
    status: "up_to_date",
    watching: false,
    file_count: 10,
    last_scan_at: "2026-11-19T10:00:00Z",
    progress: null,
    include_label: null,
    exclusion_count: 0,
    is_drive_root: false,
    is_network: false,
    parent_scope_id: null,
    error_code: null,
    ...partial,
  };
}

function snap(overrides?: Partial<SettingsSnapshot>): SettingsSnapshot {
  return {
    settings: { ...DEFAULT_SETTINGS },
    hotkey_active: true,
    os: "windows",
    ...overrides,
  };
}

async function axeViolations(container: HTMLElement) {
  const r = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } },
    resultTypes: ["violations"],
  });
  return r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  sessionStorage.setItem("tamgabul.welcomed", "1");
  window.location.hash = "";
});
afterEach(cleanup);

describe("P5 settings & hotkey conflict", () => {
  it("surfaces OS hotkey conflict, handles HOTKEY_CONFLICT on rebind, and uses macOS Cmd labels", async () => {
    const api = new FakeApi();
    const updates: AppSettings[] = [];
    const onUpdate = vi.fn(async (next: AppSettings): Promise<SettingsSnapshot> => {
      updates.push(next);
      if (next.hotkey === "Alt+K") {
        throw { code: "HOTKEY_CONFLICT", detail: "taken" };
      }
      return snap({ settings: next, hotkey_active: true });
    });

    window.location.hash = "#/settings";
    render(<Root surface="app" api={api} initialSnapshot={snap({ hotkey_active: false })} onUpdateSettings={onUpdate} />);

    expect(await screen.findByText("The current shortcut is taken by another app. Choose another.")).toBeTruthy();
    expect(screen.getByText("Apache-2.0")).toBeTruthy();

    const input = screen.getByLabelText("Widget shortcut");
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    fireEvent.keyDown(input, { key: "k", altKey: true });
    expect(await screen.findByText("Alt+K is used by another app. Choose another.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    fireEvent.keyDown(input, { key: "j", ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(updates.some((u) => u.hotkey === "Ctrl+Shift+J")).toBe(true));
    expect(screen.queryByText(/used by another app/)).toBeNull();

    const base = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
    expect(comboFromEvent({ ...base, key: " ", metaKey: true, shiftKey: true }, "macos")).toBe("Cmd+Shift+Space");
    expect(comboFromEvent({ ...base, key: "k", metaKey: true }, "windows")).toBe("Win+K");
  });
});

describe("P5 model status semantics & validation dialog", () => {
  it("shows distinct amber On disk vs green Ready/Idle/Busy vs red Error/Incompatible with text labels", async () => {
    const api = new FakeApi();
    api.models = [
      makeModel({ model_id: "m-emb", category: "embedding", status: "AVAILABLE", tone: "amber" }),
      makeModel({ model_id: "m-llm", category: "llm", status: "READY", tone: "green", memory_mb: 2400 }),
    ];

    window.location.hash = "#/models";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    const onDisk = await screen.findByText("On disk");
    expect(onDisk.className).toContain("pill-warn");
    expect(onDisk.className).not.toContain("pill-ok");

    const ready = screen.getByText("Ready");
    expect(ready.className).toContain("pill-ok");
    expect(screen.getAllByText("Optional").length).toBe(2);
    expect(await axeViolations(container)).toEqual([]);

    api.models = [
      makeModel({ model_id: "m-emb", category: "embedding", status: "ERROR", tone: "red", error_code: "RUNTIME_LOAD_FAILED" }),
      makeModel({ model_id: "m-llm", category: "llm", status: "INCOMPATIBLE", tone: "red" }),
    ];
    api.notify();
    expect(await screen.findByText("Error")).toBeTruthy();
    expect(screen.getByText("Incompatible")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Load" })).toBeNull();
  });

  it("validates before Save and uses macOS brew guidance when os is macos", async () => {
    const api = new FakeApi();
    api.validateImpl = async (_req, onStep) => {
      onStep([
        { id: "readable", label_key: "addModel.step.readable", state: "ok", detail: null },
        { id: "runtime", label_key: "addModel.step.runtime", state: "failed", detail: "addModel.fail.runtime" },
      ]);
      return false;
    };

    window.location.hash = "#/models";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap({ os: "macos" })} />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Add model" }))[0]);

    const pathInput = await screen.findByPlaceholderText("~/Models/…");
    fireEvent.change(pathInput, { target: { value: "/Users/me/models/eg2" } });
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));

    expect(await screen.findByText(/brew install llama\.cpp/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    expect(await axeViolations(container)).toEqual([]);

    api.validateImpl = async (_req, onStep) => {
      onStep([
        { id: "readable", label_key: "addModel.step.readable", state: "ok", detail: null },
        { id: "runtime", label_key: "addModel.step.runtime", state: "ok", detail: "llama.cpp b12024" },
      ]);
      return true;
    };
    fireEvent.change(pathInput, { target: { value: "/Users/me/models/eg2-valid" } });
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));

    const saveBtn = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
    await waitFor(() => expect(saveBtn.disabled).toBe(false));
    fireEvent.click(saveBtn);
    await waitFor(() => expect(api.registeredRequests).toHaveLength(1));
  });
});

describe("P5 onboarding progression", () => {
  it("enables search when embedding is AVAILABLE and warns on hotkey conflict", async () => {
    const api = new FakeApi();
    api.models = [makeModel({ model_id: "m-emb", category: "embedding", status: "AVAILABLE", tone: "amber" })];
    window.location.hash = "#/welcome";

    render(<Root surface="app" api={api} initialSnapshot={snap({ hotkey_active: false })} />);
    const startBtn = (await screen.findByRole("button", { name: "Start searching" })) as HTMLButtonElement;
    await waitFor(() => expect(startBtn.disabled).toBe(false));
    expect(screen.getByText(/Taken by another app/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Load" })).toBeTruthy();
  });
});

describe("P5 folder drive-root preview & child-under-parent revoke", () => {
  it("requires explicit risk confirmation for C:\\ drive root and completes grant", async () => {
    const api = new FakeApi();
    window.location.hash = "#/folders";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    fireEvent.click((await screen.findAllByRole("button", { name: "Add folder" }))[0]);
    const folderInput = await screen.findByLabelText("Folder");
    fireEvent.change(folderInput, { target: { value: "C:\\" } });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    expect(await screen.findByText("Add the whole C:\\ drive?")).toBeTruthy();
    const grantBtn = screen.getByRole("button", { name: "Allow and scan" }) as HTMLButtonElement;
    expect(grantBtn.disabled).toBe(true);
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("checkbox"));
    expect(grantBtn.disabled).toBe(false);
    fireEvent.click(grantBtn);
    await waitFor(() => expect(api.grantedTokens).toEqual(["tok-C:\\"]));
  });

  it("handles child-under-parent revoke dialog with Windows backslashes and accessibility", async () => {
    const api = new FakeApi();
    api.scopes = [
      makeScope({ scope_id: "s-parent", display_path: "C:\\Projects" }),
      makeScope({ scope_id: "s-child", display_path: "C:\\Projects\\sub", parent_scope_id: null }),
    ];
    api.stats = { scope_count: 2, file_count: 20, index_mb: 0.4 };

    window.location.hash = "#/folders";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Actions for C:\\Projects\\sub" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove & purge…" }));

    expect(await screen.findByText(/also inside the allowed folder C:\\Projects/)).toBeTruthy();
    expect(screen.getByLabelText(/Block and purge/)).toBeTruthy();
    expect(screen.getByLabelText(/Remove the separate grant only/)).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Remove and purge" }));
    await waitFor(() =>
      expect(api.revokedRequests).toEqual([
        { scope_id: "s-child", mode: "block_and_purge", purge_history: true },
      ]),
    );
  });
});

describe("P5 widget surface states", () => {
  it("renders direct answer, results, clarify choices, not_found, and error states", async () => {
    const api = new FakeApi();
    api.models = [makeModel({ model_id: "m-emb", category: "embedding", status: "AVAILABLE", tone: "amber" })];
    api.scopes = [makeScope({ scope_id: "s-1", display_path: "C:\\Docs" })];

    api.searchImpl = async (req) => ({
      session_id: "c-1",
      turn_id: "a-1",
      kind: "results",
      count: 5,
      choices: [],
      results: [1, 2, 3, 4].map((i) => ({
        result_ref: `r:${i}`,
        title: `doc_${i}.pdf`,
        display_path: `C:\\Docs\\doc_${i}.pdf`,
        project: "Docs",
        file_kind: "pdf",
        snippet: `Excerpt ${i}`,
        highlights: [],
        modified_at: "2026-11-18T10:00:00Z",
        match_reason: "semantic",
      })),
      question: null,
      understood: null,
      answer: req.text.includes("visa") ? { text: "Issued on 12 March 2024.", result_ref: "r:1" } : null,
      summary: null,
      picked_by_llm: true,
      picked_count: 1,
      notice: null,
      degraded_llm: false,
      searched_scope_count: 1,
    });

    render(<Root surface="widget" api={api} initialSnapshot={snap()} />);
    const input = await screen.findByPlaceholderText("Ask about a file, note or past conversation...");
    expect((screen.getByRole("button", { name: "Voice search needs an STT model" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(input, { target: { value: "when was my visa" } });
    fireEvent.submit(input.closest("form")!);

    expect(await screen.findByText("Issued on 12 March 2024.")).toBeTruthy();
    expect(screen.getByText("doc_1.pdf")).toBeTruthy();
    expect(screen.getByRole("button", { name: "See all 5 in the app" })).toBeTruthy();

    api.searchImpl = async () => ({
      session_id: "c-1",
      turn_id: "a-2",
      kind: "clarify",
      count: 0,
      choices: [{ id: "opt:0", label: "20coin", count: 2 }, { id: "all", label: "all", count: 0 }],
      results: [],
      question: "Which project?",
      understood: null,
      answer: null,
      summary: null,
      picked_by_llm: false,
      picked_count: 0,
      notice: null,
      degraded_llm: false,
      searched_scope_count: 1,
    });
    fireEvent.change(input, { target: { value: "that bug" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("Which project?")).toBeTruthy();
    expect(screen.getByRole("button", { name: "20coin" })).toBeTruthy();

    api.searchImpl = async () => ({
      session_id: "c-1",
      turn_id: "a-3",
      kind: "not_found",
      count: 0,
      choices: [],
      results: [],
      question: null,
      understood: null,
      answer: null,
      summary: null,
      picked_by_llm: false,
      picked_count: 0,
      notice: null,
      degraded_llm: true,
      searched_scope_count: 1,
    });
    fireEvent.change(input, { target: { value: "mars colony" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("I couldn't find a match.")).toBeTruthy();

    api.searchImpl = async () => {
      throw { code: "INDEX_UNAVAILABLE", detail: "busy" };
    };
    fireEvent.change(input, { target: { value: "fail" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByRole("alert")).toBeTruthy();
  });
});

describe("P5 small feature gaps: exclusions, network folders, explicit downloader", () => {
  it("opens per-folder exclusions dialog, lists/adds/removes rules, and passes accessibility", async () => {
    const api = new FakeApi();
    api.scopes = [
      makeScope({ scope_id: "s-1", display_path: "C:\\Projects", exclusion_count: 1 }),
    ];
    api.exclusionsByScope["s-1"] = [
      {
        exclusion_id: "x-block",
        scope_id: "s-1",
        display_path: "C:\\Projects\\legacy",
        reason: "child_revoked",
        created_at: "2026-11-19T09:00:00Z",
      },
    ];

    window.location.hash = "#/folders";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Actions for C:\\Projects" }));
    fireEvent.click(screen.getByRole("button", { name: "Exclusions (1)" }));

    expect(await screen.findByText("Excluded folders")).toBeTruthy();
    expect(await screen.findByText("C:\\Projects\\legacy")).toBeTruthy();
    expect(screen.getByText("Blocked on removal")).toBeTruthy();
    expect(screen.getByText("node_modules")).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);

    const input = screen.getByLabelText("Exclude a subfolder");
    fireEvent.change(input, { target: { value: "private" } });
    fireEvent.click(screen.getByRole("button", { name: "Exclude" }));

    expect(await screen.findByText("C:\\Projects\\private")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Exclusions (2)" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Remove exclusion for C:\\Projects\\legacy" }));
    await waitFor(() => expect(screen.queryByText("C:\\Projects\\legacy")).toBeNull());
    expect(screen.getByRole("button", { name: "Exclusions (1)" })).toBeTruthy();
  });

  it("gates network/UNC folder grant behind explicit confirmation and displays network badge", async () => {
    const api = new FakeApi();
    window.location.hash = "#/folders";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    fireEvent.click((await screen.findAllByRole("button", { name: "Add folder" }))[0]);
    const folderInput = await screen.findByLabelText("Folder");
    fireEvent.change(folderInput, { target: { value: "\\\\nas\\docs" } });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    expect(await screen.findByText("Add network folder \\\\nas\\docs?")).toBeTruthy();
    const grantBtn = screen.getByRole("button", { name: "Allow and scan" }) as HTMLButtonElement;
    expect(grantBtn.disabled).toBe(true);
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("checkbox"));
    expect(grantBtn.disabled).toBe(false);
    fireEvent.click(grantBtn);

    expect(await screen.findByText("Network")).toBeTruthy();
  });

  it("provides consent-gated official model download and flows into local validation", async () => {
    const api = new FakeApi();
    api.downloadable = [
      {
        preset_id: "eg2-bf16",
        category: "embedding",
        display_name: "EmbeddingGemma 2 (BF16 + image/audio)",
        recommended: true,
        repo: "ggml-org/embeddinggemma-2-GGUF",
        revision: "6ea3e3a309d12a4779cfda98b8c19800d2d5da71",
        origin_host: "huggingface.co",
        license_name: "Apache-2.0",
        license_url: "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF",
        default_dir: "C:\\Users\\me\\AppData\\Roaming\\Tamgabul\\models\\embeddinggemma-2-bf16",
        total_bytes: 1_540_024_992,
        files: [
          { filename: "embeddinggemma-2-BF16.gguf", size_bytes: 557_950_176, sha256: "abc" },
          { filename: "mmproj-embeddinggemma-2-BF16.gguf", size_bytes: 982_074_816, sha256: "def" },
        ],
        capabilities: ["text", "image", "audio"],
      },
    ];
    const validatedPaths: string[] = [];
    api.validateImpl = async (req, onStep) => {
      validatedPaths.push(req.path);
      onStep([
        { id: "readable", label_key: "addModel.step.readable", state: "ok", detail: null },
        { id: "runtime", label_key: "addModel.step.runtime", state: "ok", detail: "llama.cpp b12024" },
      ]);
      return true;
    };

    window.location.hash = "#/models";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Add model" }))[0]);

    // Defaults strictly to local path mode.
    expect(await screen.findByLabelText("Model file or folder")).toBeTruthy();

    // Switching to STT hides the download toggle (no downloadable STT preset).
    fireEvent.click(screen.getByRole("radio", { name: "STT" }));
    expect(screen.queryByRole("radio", { name: "Download official model" })).toBeNull();

    // Back to Embedding -> switch to Download official model.
    fireEvent.click(screen.getByRole("radio", { name: "Embedding" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Download official model" }));

    expect(await screen.findByText(/huggingface\.co\/ggml-org\/embeddinggemma-2-GGUF/)).toBeTruthy();
    const dlBtn = screen.getByRole("button", { name: "Download" }) as HTMLButtonElement;
    expect(dlBtn.disabled).toBe(true);
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("checkbox"));
    expect(dlBtn.disabled).toBe(false);
    fireEvent.click(dlBtn);

    await waitFor(() =>
      expect(api.downloadRequests).toEqual([
        {
          preset_id: "eg2-bf16",
          destination_dir: "C:\\Users\\me\\AppData\\Roaming\\Tamgabul\\models\\embeddinggemma-2-bf16",
          consent: true,
        },
      ]),
    );
    await waitFor(() =>
      expect(validatedPaths).toEqual([
        "C:\\Users\\me\\AppData\\Roaming\\Tamgabul\\models\\embeddinggemma-2-bf16",
      ]),
    );
    const saveBtn = (await screen.findByRole("button", { name: "Save" })) as HTMLButtonElement;
    expect(saveBtn.disabled).toBe(false);
  });
});

describe("P6 voice search (STT) and read aloud (TTS)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("enables mic for AVAILABLE STT, captures, transcribes locally, allows editing, and handles mic denial gracefully", async () => {
    const wavBytes = audioLib.encodeWav16kMono(new Int16Array(320));
    vi.spyOn(audioLib, "startMicCapture").mockResolvedValue({
      stop: async () => wavBytes,
      cancel: vi.fn(),
    });

    const api = new FakeApi();
    api.models = [
      makeModel({ model_id: "m-emb", category: "embedding", status: "AVAILABLE", tone: "amber" }),
      makeModel({
        model_id: "m-stt",
        category: "stt",
        display_name: "Whisper (ggml-small-q5_1)",
        status: "AVAILABLE",
        tone: "amber",
        runtime: "whisper.cpp",
      }),
    ];
    api.scopes = [makeScope({ scope_id: "s-1", display_path: "C:\\Projects" })];
    api.transcribeImpl = async () => ({ text: "fatura nisan" });
    const submittedQueries: string[] = [];
    api.searchImpl = async (req) => {
      submittedQueries.push(req.text);
      return {
        session_id: "c-v1",
        turn_id: "a-v1",
        kind: "results",
        count: 1,
        choices: [],
        results: [
          {
            result_ref: "r-v1",
            title: "nisan-fatura.pdf",
            display_path: "C:\\Projects\\nisan-fatura.pdf",
            project: null,
            file_kind: "pdf",
            snippet: "Toplam 1250 TL",
            highlights: [],
            modified_at: "2026-11-19T10:00:00Z",
            match_reason: "transcript",
          },
        ],
        question: null,
        understood: null,
        answer: null,
        summary: null,
        picked_by_llm: false,
        picked_count: 0,
        notice: null,
        degraded_llm: true,
        searched_scope_count: 1,
      };
    };

    window.location.hash = "#/search";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);

    const micBtn = (await screen.findByRole("button", { name: "Voice search" })) as HTMLButtonElement;
    expect(micBtn.disabled).toBe(false);
    fireEvent.click(micBtn);

    const voiceDialog = await screen.findByRole("dialog", { name: "Voice search" });
    expect(voiceDialog).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(within(voiceDialog).getAllByRole("button", { name: "Stop" })[0]);
    const reviewInput = (await screen.findByLabelText("What I heard — you can edit it")) as HTMLInputElement;
    expect(reviewInput.value).toBe("fatura nisan");
    expect(api.transcribedRequests.length).toBe(1);

    fireEvent.change(reviewInput, { target: { value: "fatura nisan 2026" } });
    fireEvent.click(within(voiceDialog).getByRole("button", { name: /Search/ }));

    expect(await screen.findByText("nisan-fatura.pdf")).toBeTruthy();
    expect(submittedQueries).toEqual(["fatura nisan 2026"]);
    fireEvent.click(screen.getByRole("button", { name: /Preview/ }));
    expect(await screen.findByText("Spoken words")).toBeTruthy();

    // Microphone permission denial shows honest fallback while typed search remains enabled.
    vi.spyOn(audioLib, "startMicCapture").mockRejectedValue(new DOMException("denied", "NotAllowedError"));
    fireEvent.click(screen.getByRole("button", { name: "Voice search" }));
    expect(await screen.findByText("Microphone access was denied")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Cancel/ }));
    expect(screen.getByPlaceholderText("Ask something else...")).toBeTruthy();
  });

  it("provides stoppable Read aloud control when TTS is installed and hides it when missing", async () => {
    const playSpy = vi.fn().mockResolvedValue(undefined);
    const pauseSpy = vi.fn();
    class FakeAudio {
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor(public src: string) {}
      play() { return playSpy(); }
      pause() { pauseSpy(); }
    }
    vi.stubGlobal("Audio", FakeAudio);

    const api = new FakeApi();
    api.models = [
      makeModel({ model_id: "m-emb", category: "embedding", status: "AVAILABLE", tone: "amber" }),
      makeModel({
        model_id: "m-tts",
        category: "tts",
        display_name: "Piper (tr_TR-dfki-medium)",
        status: "AVAILABLE",
        tone: "amber",
        runtime: "piper",
      }),
    ];
    api.scopes = [makeScope({ scope_id: "s-1", display_path: "C:\\Projects" })];
    api.searchImpl = async () => ({
      session_id: "c-t1",
      turn_id: "a-t1",
      kind: "results",
      count: 1,
      choices: [],
      results: [
        {
          result_ref: "r-t1",
          title: "vize.jpg",
          display_path: "C:\\Projects\\vize.jpg",
          project: null,
          file_kind: "image",
          snippet: null,
          highlights: [],
          modified_at: "2026-11-19T10:00:00Z",
          match_reason: "caption",
        },
      ],
      question: null,
      understood: null,
      answer: { text: "UK vizen 12 Mart 2024 tarihli.", result_ref: "r-t1" },
      summary: null,
      picked_by_llm: true,
      picked_count: 1,
      notice: null,
      degraded_llm: false,
      searched_scope_count: 1,
    });

    window.location.hash = "#/search";
    render(<Root surface="app" api={api} initialSnapshot={snap()} />);
    const input = await screen.findByPlaceholderText(/Ask about a file/);
    fireEvent.change(input, { target: { value: "uk vize" } });
    fireEvent.submit(input.closest("form")!);

    const readBtn = await screen.findByRole("button", { name: "Read aloud" });
    fireEvent.click(readBtn);

    await waitFor(() =>
      expect(api.synthesizedRequests).toEqual([{ text: "UK vizen 12 Mart 2024 tarihli." }]),
    );
    expect(playSpy).toHaveBeenCalledTimes(1);

    const stopBtn = await screen.findByRole("button", { name: "Stop reading" });
    fireEvent.click(stopBtn);
    await waitFor(() => expect(api.stopSpeechCalls).toBeGreaterThanOrEqual(1));
    expect(pauseSpy).toHaveBeenCalledTimes(1);

    // Removing TTS hides the control cleanly.
    await api.removeModel("m-tts");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Read aloud" })).toBeNull());
  });

  it("displays verified STT & TTS download presets including Turkish CC-BY-NC-SA-4.0 dataset notice", async () => {
    const api = new FakeApi();
    api.downloadable = [
      {
        preset_id: "whisper-small-q5_1",
        category: "stt",
        display_name: "Whisper small (Q5_1 · TR + EN)",
        recommended: true,
        repo: "ggerganov/whisper.cpp",
        revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
        origin_host: "huggingface.co",
        license_name: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp",
        default_dir: "C:\\models\\whisper-small-q5_1",
        total_bytes: 190_085_487,
        files: [{ filename: "ggml-small-q5_1.bin", size_bytes: 190_085_487, sha256: "ae85" }],
        capabilities: ["voice_input", "audio_transcript", "tr", "en"],
      },
      {
        preset_id: "piper-tr-dfki-medium",
        category: "tts",
        display_name: "Piper Turkish (dfki medium · non-commercial dataset)",
        recommended: true,
        repo: "rhasspy/piper-voices",
        revision: "04ca88f5685ca5253c701af3687e155b113bd512",
        origin_host: "huggingface.co",
        license_name: "CC-BY-NC-SA-4.0 (dataset) / MIT (weights card)",
        license_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/tr/tr_TR/dfki/medium/MODEL_CARD",
        default_dir: "C:\\models\\piper-tr-dfki-medium",
        total_bytes: 63_206_254,
        files: [
          { filename: "tr_TR-dfki-medium.onnx", size_bytes: 63_201_294, sha256: "2844" },
          { filename: "tr_TR-dfki-medium.onnx.json", size_bytes: 4_960, sha256: "13eb" },
        ],
        capabilities: ["read_aloud", "tr"],
      },
    ];

    window.location.hash = "#/models";
    const { container } = render(<Root surface="app" api={api} initialSnapshot={snap()} />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Add model" }))[0]);

    fireEvent.click(await screen.findByRole("radio", { name: "STT" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Download official model" }));
    expect(await screen.findByText(/ggerganov\/whisper\.cpp/)).toBeTruthy();

    fireEvent.click(screen.getByRole("radio", { name: "TTS" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Download official model" }));
    expect(
      (await screen.findAllByText(/CC-BY-NC-SA-4\.0 \(dataset\)/)).length,
    ).toBeGreaterThanOrEqual(2);
    expect(await axeViolations(container)).toEqual([]);
  });
});
