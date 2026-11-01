// MOCK ONLY demo backend. Everything here is invented sample data so the UI can
// be built and reviewed before the real backend phases (P1–P4). The UI shows a
// "MOCK ONLY" badge whenever this store is active. State is kept in
// localStorage so the widget and app windows see the same demo session.
import type { Api } from "./api";
import type { DemoScenario } from "../bindings/DemoScenario";
import type { HistorySession } from "../bindings/HistorySession";
import type { HistoryTurn } from "../bindings/HistoryTurn";
import type { IpcError } from "../bindings/IpcError";
import type { ModelCategory } from "../bindings/ModelCategory";
import type { ModelEntry } from "../bindings/ModelEntry";
import type { ResultCard } from "../bindings/ResultCard";
import type { ScopePreview } from "../bindings/ScopePreview";
import type { ScopeSummary } from "../bindings/ScopeSummary";
import type { SearchResponse } from "../bindings/SearchResponse";
import type { ValidationStep } from "../bindings/ValidationStep";
import { deriveHealth, toneOf } from "../lib/health";

const KEY = "tamgabul.mock.v1";
const DAY = 86_400_000;
const iso = (msAgo: number) => new Date(Date.now() - msAgo).toISOString();
const uid = (p: string) => `${p}-${Math.random().toString(36).slice(2, 9)}`;
const err = (code: IpcError["code"], detail: string): IpcError => ({ code, detail });

type Session = HistorySession & { turns: HistoryTurn[]; pending?: string[] };
type Corpus = ResultCard & { body: string; keywords: string[] };
type State = {
  scenario: DemoScenario;
  models: ModelEntry[];
  scopes: ScopeSummary[];
  sessions: Session[];
};

// ---------------------------------------------------------------- sample corpus

const CORPUS: Corpus[] = [
  {
    result_ref: "r-auction", title: "auction-timer-bug.md", display_path: "C:/Projects/20coin-clash/docs/bugs",
    project: "20coin Clash", file_kind: "markdown", modified_at: iso(2 * DAY), match_reason: "previous_result",
    snippet: "Auction timer sometimes drops below zero in the last 10 seconds. With network latency the countdown sync breaks. Temporary fix: clamp to a minimum of 0 on the client...",
    highlights: ["Auction timer"], keywords: ["timer", "auction", "countdown", "zamanlayıcı"],
    body: "# Auction timer bug\n\nAuction timer sometimes drops below zero in the last 10 seconds.\n\nWith network latency the countdown sync breaks: the client keeps its own tick while the server already closed the round.\n\n## Temporary fix\nClamp the displayed value to a minimum of 0 on the client and resync on the next server heartbeat.\n\n## Next\n- Move the countdown to server time\n- Add a test with 300 ms injected latency",
  },
  {
    result_ref: "r-timer-png", title: "timer_bug_screen.png", display_path: "C:/Projects/MODU/screenshots",
    project: "MODU", file_kind: "image", modified_at: iso(9 * DAY), match_reason: "ocr",
    snippet: "Text found in image: \"00:-03 Round ended\"", highlights: ["00:-03"], keywords: ["timer", "screenshot", "image"],
    body: "Screenshot. Text found in image:\n\n\"00:-03\"\n\"Round ended\"\n\"Retry\"",
  },
  {
    result_ref: "r-buy-button", title: "game-ui-buttons.md", display_path: "C:/Projects/20coin-clash/docs/ui",
    project: "20coin Clash", file_kind: "markdown", modified_at: iso(6 * DAY), match_reason: "semantic",
    snippet: "The Buy button on the game screen stops responding after the round ends until the lobby reloads.",
    highlights: ["Buy button"], keywords: ["button", "buton", "game", "oyun"],
    body: "# Game UI buttons\n\nThe Buy button on the game screen stops responding after the round ends until the lobby reloads.\n\nCause: the click handler is bound to the old round object.",
  },
  {
    result_ref: "r-lobby-button", title: "lobby-button-state.md", display_path: "C:/Projects/20coin-clash/src/notes",
    project: "20coin Clash", file_kind: "markdown", modified_at: iso(12 * DAY), match_reason: "semantic",
    snippet: "Ready button stays disabled when a player rejoins the lobby.", highlights: ["Ready button"],
    keywords: ["button", "buton", "lobby"],
    body: "# Lobby button state\n\nReady button stays disabled when a player rejoins the lobby.",
  },
  {
    result_ref: "r-modu-button", title: "onboarding-button.md", display_path: "C:/Projects/MODU/docs",
    project: "MODU", file_kind: "markdown", modified_at: iso(20 * DAY), match_reason: "semantic",
    snippet: "Continue button does nothing on small screens; it is hidden under the keyboard.", highlights: ["Continue button"],
    keywords: ["button", "buton", "screen", "ekran"],
    body: "# Onboarding button\n\nContinue button does nothing on small screens; it is hidden under the keyboard.",
  },
  {
    result_ref: "r-sprint", title: "sprint-planning-october.md", display_path: "D:/Notes/work",
    project: null, file_kind: "markdown", modified_at: iso(1 * DAY), match_reason: "semantic",
    snippet: "Sprint goals: auction fixes, lobby polish, model manager spike.", highlights: ["Sprint goals"],
    keywords: ["sprint", "planning", "plan"],
    body: "# Sprint planning — October\n\nSprint goals: auction fixes, lobby polish, model manager spike.\n\nOwners and dates are in the board.",
  },
  {
    result_ref: "r-meeting", title: "meeting-notes.md", display_path: "D:/Notes/meetings",
    project: null, file_kind: "markdown", modified_at: iso(1 * DAY), match_reason: "semantic",
    snippet: "Decisions: ship the widget first, settings later. Follow up on the installer size.", highlights: ["Decisions"],
    keywords: ["meeting", "toplantı", "notes", "yesterday", "dünkü"],
    body: "# Meeting notes\n\nDecisions: ship the widget first, settings later.\n\nFollow up on the installer size.",
  },
  {
    result_ref: "r-react", title: "react-hooks-cheatsheet.md", display_path: "D:/Notes/dev",
    project: null, file_kind: "markdown", modified_at: iso(15 * DAY), match_reason: "semantic",
    snippet: "useEffect runs after paint; pass a dependency list to control when it re-runs.", highlights: ["useEffect"],
    keywords: ["react", "hooks", "useeffect"],
    body: "# React hooks\n\nuseEffect runs after paint; pass a dependency list to control when it re-runs.\n\nuseMemo caches a value between renders.",
  },
  {
    result_ref: "r-react-pdf", title: "react-query-notes.pdf", display_path: "D:/Notes/dev",
    project: null, file_kind: "pdf", modified_at: iso(30 * DAY), match_reason: "semantic",
    snippet: "Queries are cached by key; invalidate after a mutation.", highlights: ["invalidate"],
    keywords: ["react", "query"],
    body: "React Query notes (PDF, page 1)\n\nQueries are cached by key; invalidate after a mutation.",
  },
  {
    result_ref: "r-red-error", title: "red-error-dialog.png", display_path: "C:/Users/Me/Documents/Screenshots",
    project: null, file_kind: "image", modified_at: iso(5 * DAY), match_reason: "ocr",
    snippet: "Text found in image: \"Connection lost — retry?\"", highlights: ["Connection lost"],
    keywords: ["error", "hata", "screenshot", "red", "kırmızı"],
    body: "Screenshot. Text found in image:\n\n\"Connection lost — retry?\"",
  },
];

const card = (c: Corpus): ResultCard => {
  const { body: _b, keywords: _k, ...rest } = c;
  return rest;
};
const byRef = (ref: string) => CORPUS.find((c) => c.result_ref === ref);

// ---------------------------------------------------------------- seeds

function model(
  category: ModelCategory, display_name: string, source_label: string | null,
  status: ModelEntry["status"], extra: Partial<ModelEntry> = {},
): ModelEntry {
  return {
    model_id: `m-${category}`, category, display_name, source_label, status,
    tone: toneOf(status, category), required: category === "embedding",
    runtime: null, dimension: null, memory_mb: null, validated_at: iso(3 * 3_600_000),
    ttl: "minutes10", error_code: null, error_key: null, ...extra,
  };
}

function seedModels(s: DemoScenario): ModelEntry[] {
  if (s === "first_run") return [];
  return [
    model("embedding", "EmbeddingGemma 2", "google/embeddinggemma-2", "READY", { dimension: 768, ttl: "minutes30" }),
    model("llm", "Gemma 4 E4B", "Q4_K_M · GGUF", s === "llm_off" ? "AVAILABLE" : "READY", { runtime: "llama.cpp" }),
    model("stt", "Whisper (base)", "whisper-base", "READY", { ttl: "minutes5" }),
    model("tts", "Piper (tr_TR)", "tr_TR-voice", "INCOMPATIBLE", {
      error_code: "MODEL_INCOMPATIBLE", error_key: "model.err.voice_profile_missing", validated_at: null,
    }),
  ];
}

function scope(id: string, path: string, extra: Partial<ScopeSummary>): ScopeSummary {
  return {
    scope_id: id, display_path: path, status: "up_to_date", watching: true, file_count: null,
    last_scan_at: null, progress: null, include_label: null, exclusion_count: 0, is_drive_root: false,
    parent_scope_id: null, ...extra,
  };
}

function seedScopes(s: DemoScenario): ScopeSummary[] {
  if (s === "first_run") return [];
  return [
    scope("s-projects", "C:/Projects", { file_count: 1284, last_scan_at: iso(4 * 3_600_000), exclusion_count: 2 }),
    scope("s-notes", "D:/Notes", { status: "scanning", progress: 0.42, file_count: 312, include_label: ".md, .txt, .pdf, images", exclusion_count: 3 }),
    scope("s-docs", "C:/Users/Me/Documents", { status: "paused", watching: false, file_count: 540, last_scan_at: iso(6 * DAY) }),
    scope("s-archive", "E:/Archive", { status: "unreachable", file_count: 2210, last_scan_at: iso(40 * DAY) }),
  ];
}

function sessionWith(title: string, ago: number, query: string, refs: string[], opened: boolean): Session {
  const id = uid("sess");
  const results = refs.map((r) => card(byRef(r)!));
  return {
    session_id: id, title, started_at: iso(ago), result_count: results.length, opened_count: opened ? 1 : 0,
    turns: [
      { turn_id: uid("t"), role: "user", at: iso(ago), text: query, kind: null, count: null, choices: [], results: [], opened: false },
      { turn_id: uid("t"), role: "assistant", at: iso(ago - 2000), text: null, kind: "results", count: results.length, choices: [], results, opened },
    ],
  };
}

function seedSessions(s: DemoScenario): Session[] {
  if (s === "first_run") return [];
  return [
    sessionWith("Sprint planning notes", 1 * DAY, "sprint planning notes", ["r-sprint", "r-meeting"], false),
    sessionWith("Auction timer bug from last week", 2 * DAY, "the auction timer bug I found last week", ["r-auction"], true),
    sessionWith("Red error in a screenshot", 5 * DAY, "screenshot with a red error", ["r-red-error", "r-timer-png"], false),
  ];
}

const seed = (scenario: DemoScenario): State => ({
  scenario,
  models: seedModels(scenario),
  scopes: seedScopes(scenario),
  sessions: seedSessions(scenario),
});

// ---------------------------------------------------------------- store

const STEP_MS = 450;
const pause = (ms: number, signal?: AbortSignal) =>
  new Promise<void>((res, rej) => {
    const t = setTimeout(res, ms);
    signal?.addEventListener("abort", () => { clearTimeout(t); rej(err("USER_CANCELLED", "cancelled")); });
  });

function hash(s: string): number {
  let h = 0;
  for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return h;
}

export class MockApi implements Api {
  readonly mode = "mock" as const;
  private state: State;
  private listeners = new Set<() => void>();
  private previews = new Map<string, ScopePreview>();
  private timers = new Map<string, ReturnType<typeof setInterval>>();

  constructor(scenario: DemoScenario) {
    this.state = this.read() ?? seed(scenario);
    if (this.state.scenario !== scenario) this.state = seed(scenario);
    this.write();
    if (typeof window !== "undefined") {
      window.addEventListener("storage", this.onStorage);
    }
    // Resume animations that were in progress (e.g. after a reload).
    for (const s of this.state.scopes) if (s.status === "scanning") this.animateScan(s.scope_id);
  }

  dispose() {
    window.removeEventListener("storage", this.onStorage);
    this.timers.forEach((t) => clearInterval(t));
  }

  /** Throw away demo data and start the scenario fresh. */
  reset(scenario: DemoScenario) {
    this.state = seed(scenario);
    this.commit();
  }

  private onStorage = (e: StorageEvent) => {
    if (e.key !== KEY || !e.newValue) return;
    try {
      this.state = JSON.parse(e.newValue) as State;
      this.emit();
    } catch { /* ignore foreign writes */ }
  };

  private read(): State | null {
    try {
      const raw = localStorage.getItem(KEY);
      return raw ? (JSON.parse(raw) as State) : null;
    } catch { return null; }
  }
  private write() {
    try { localStorage.setItem(KEY, JSON.stringify(this.state)); } catch { /* memory only */ }
  }
  private emit() { this.listeners.forEach((l) => l()); }
  private commit() { this.write(); this.emit(); }

  subscribe(cb: () => void) {
    this.listeners.add(cb);
    return () => { this.listeners.delete(cb); };
  }

  private model(id: string) {
    const m = this.state.models.find((x) => x.model_id === id);
    if (!m) throw err("MODEL_MISSING", "unknown model");
    return m;
  }
  private setStatus(m: ModelEntry, status: ModelEntry["status"]) {
    m.status = status;
    m.tone = toneOf(status, m.category);
  }
  private scope(id: string) {
    const s = this.state.scopes.find((x) => x.scope_id === id);
    if (!s) throw err("SCOPE_NOT_AUTHORIZED", "unknown scope");
    return s;
  }
  private activeScopes() {
    return this.state.scopes.filter((s) => s.status !== "revoking" && s.status !== "unreachable");
  }

  // ------------------------------------------------------------ health & models

  async health() {
    return deriveHealth(this.state.models, this.activeScopes().length);
  }

  async listModels() {
    return structuredClone(this.state.models);
  }

  async loadModel(id: string) {
    const m = this.model(id);
    if (m.status === "INCOMPATIBLE") throw err("MODEL_INCOMPATIBLE", "cannot load");
    this.setStatus(m, "LOADING");
    this.commit();
    await pause(900);
    this.setStatus(this.model(id), "READY");
    this.commit();
  }

  async unloadModel(id: string) {
    const m = this.model(id);
    this.setStatus(m, "UNLOADING");
    this.commit();
    await pause(400);
    this.setStatus(this.model(id), "AVAILABLE");
    this.commit();
  }

  async unloadAll() {
    await Promise.all(this.state.models.filter((m) => m.tone === "green").map((m) => this.unloadModel(m.model_id)));
  }

  async setTtl(id: string, ttl: ModelEntry["ttl"]) {
    this.model(id).ttl = ttl;
    this.commit();
  }

  async removeModel(id: string) {
    this.state.models = this.state.models.filter((m) => m.model_id !== id);
    this.commit();
  }

  async validateModel(req: { category: ModelCategory; path: string }, onStep: (s: ValidationStep[]) => void, signal: AbortSignal) {
    const ext = req.path.toLowerCase();
    const steps: ValidationStep[] = [
      { id: "readable", label_key: "addModel.step.readable", state: "pending", detail: null },
      { id: "format", label_key: `addModel.step.format.${req.category}`, state: "pending", detail: null },
      { id: "runtime", label_key: "addModel.step.runtime", state: "pending", detail: null },
      { id: "smoke", label_key: "addModel.step.smoke", state: "pending", detail: null },
    ];
    const failAt =
      !req.path.trim() ? 0
      : req.category === "llm" && !ext.endsWith(".gguf") ? 1
      : req.category === "tts" && !ext.endsWith(".onnx") ? 1
      : -1;
    for (let i = 0; i < steps.length; i++) {
      steps[i].state = "running";
      onStep(structuredClone(steps));
      await pause(STEP_MS, signal);
      if (i === failAt) {
        steps[i].state = "failed";
        steps[i].detail = `addModel.fail.${steps[i].id}.${req.category}`;
        onStep(structuredClone(steps));
        return false;
      }
      steps[i].state = "ok";
      if (i === 0) steps[i].detail = "addModel.detail.size";
      onStep(structuredClone(steps));
    }
    return true;
  }

  async registerModel(req: { category: ModelCategory; path: string }) {
    const file = req.path.split(/[\\/]/).filter(Boolean).pop() ?? req.path;
    const name = req.category === "embedding" ? "EmbeddingGemma 2" : file.replace(/\.(gguf|onnx|bin)$/i, "");
    const m = model(req.category, name, file, "AVAILABLE", {
      model_id: uid("m"), runtime: req.category === "llm" ? "llama.cpp" : null,
      dimension: req.category === "embedding" ? 768 : null, validated_at: new Date().toISOString(),
    });
    this.state.models = [...this.state.models.filter((x) => x.category !== req.category), m];
    this.commit();
    return m;
  }

  // ------------------------------------------------------------ folders

  async listScopes() { return structuredClone(this.state.scopes); }

  async indexStats() {
    const live = this.state.scopes.filter((s) => s.status !== "revoking");
    const files = live.reduce((n, s) => n + (s.file_count ?? 0), 0);
    return { scope_count: live.length, file_count: live.length ? files : null, index_mb: live.length ? Math.round(files * 0.012 * 10) / 10 : null };
  }

  async previewScope(path: string) {
    const clean = path.replace(/\\/g, "/").replace(/\/$/, "");
    if (!clean) throw err("SCOPE_NOT_AUTHORIZED", "empty path");
    const isRoot = /^[A-Za-z]:\/?$/.test(clean) || clean === "";
    const parent = this.state.scopes.find((s) => clean.startsWith(s.display_path + "/"));
    const h = hash(clean);
    const pv: ScopePreview = {
      preview_token: `pv-${h}`,
      display_path: isRoot ? clean.slice(0, 2) + "/" : clean,
      is_drive_root: isRoot,
      file_count: isRoot ? 180_000 + (h % 60_000) : 80 + (h % 2400),
      est_index_mb: isRoot ? 2400 + (h % 900) : 2 + (h % 40),
      cap_files: isRoot ? 250_000 : 50_000,
      auto_excludes: isRoot
        ? ["Windows", "Program Files", "ProgramData", "AppData", "$Recycle.Bin", "node_modules", ".git"]
        : ["node_modules", ".git", ".venv"],
      parent_display_path: parent?.display_path ?? null,
    };
    this.previews.set(pv.preview_token, pv);
    return pv;
  }

  async grantScope(token: string) {
    const pv = this.previews.get(token);
    if (!pv) throw err("SCOPE_NOT_AUTHORIZED", "preview expired");
    const s = scope(uid("s"), pv.display_path, {
      status: "scanning", progress: 0, file_count: 0, is_drive_root: pv.is_drive_root,
      exclusion_count: pv.auto_excludes.length,
      parent_scope_id: this.state.scopes.find((x) => x.display_path === pv.parent_display_path)?.scope_id ?? null,
    });
    this.state.scopes.push(s);
    this.commit();
    this.animateScan(s.scope_id, pv.file_count);
    return s;
  }

  private animateScan(id: string, total?: number) {
    if (this.timers.has(id)) return;
    const t = setInterval(() => {
      const s = this.state.scopes.find((x) => x.scope_id === id);
      if (!s || s.status !== "scanning") { clearInterval(t); this.timers.delete(id); return; }
      const p = Math.min(1, (s.progress ?? 0) + 0.04);
      s.progress = p;
      if (total) s.file_count = Math.round(total * p);
      if (p >= 1) {
        s.status = s.watching ? "up_to_date" : "paused";
        s.progress = null;
        s.last_scan_at = new Date().toISOString();
        clearInterval(t);
        this.timers.delete(id);
      }
      this.commit();
    }, 500);
    this.timers.set(id, t);
  }

  async setWatching(id: string, on: boolean) {
    const s = this.scope(id);
    s.watching = on;
    if (s.status !== "scanning") s.status = on ? "up_to_date" : "paused";
    this.commit();
  }

  async startIndex(id: string) {
    const s = this.scope(id);
    if (s.status === "unreachable") throw err("SCOPE_REVOKED", "path unreachable");
    s.status = "scanning";
    s.progress = 0;
    this.commit();
    this.animateScan(id);
  }

  async cancelIndex(id: string) {
    const s = this.scope(id);
    s.status = s.watching ? "up_to_date" : "paused";
    s.progress = null;
    this.commit();
  }

  async relinkScope(id: string) {
    const s = this.scope(id);
    s.status = "up_to_date";
    s.last_scan_at = new Date().toISOString();
    this.commit();
  }

  async revokeScope(req: { scope_id: string; mode: string; purge_history: boolean }) {
    const s = this.scope(req.scope_id);
    if (req.mode === "stop_watching") return this.setWatching(s.scope_id, false);
    s.status = "revoking";
    this.commit();
    await pause(1200);
    const path = s.display_path;
    this.state.scopes = this.state.scopes.filter((x) => x.scope_id !== req.scope_id);
    // Revoked content disappears from history results immediately (ADR 015).
    for (const sess of this.state.sessions) {
      for (const turn of sess.turns) turn.results = turn.results.filter((r) => !r.display_path.startsWith(path));
    }
    if (req.purge_history) {
      this.state.sessions = this.state.sessions.filter((x) => x.turns.some((t) => t.results.length || t.role === "user"));
    }
    this.commit();
  }

  // ------------------------------------------------------------ search

  private searchable() {
    const paths = this.activeScopes().map((s) => s.display_path);
    return CORPUS.filter((c) => paths.some((p) => c.display_path.startsWith(p)));
  }

  async search(req: { session_id: string | null; text: string; choice_id: string | null }): Promise<SearchResponse> {
    const emb = this.state.models.find((m) => m.category === "embedding");
    if (!emb || toneOf(emb.status, "embedding") === "red") throw err("MODEL_MISSING", "embedding model required");
    await pause(350);

    let sess = this.state.sessions.find((x) => x.session_id === req.session_id);
    if (!sess) {
      sess = { session_id: uid("sess"), title: req.text.slice(0, 60), started_at: new Date().toISOString(), result_count: 0, opened_count: 0, turns: [] };
      this.state.sessions.unshift(sess);
    }
    const now = new Date().toISOString();
    sess.turns.push({ turn_id: uid("t"), role: "user", at: now, text: req.text, kind: null, count: null, choices: [], results: [], opened: false });

    const pool = this.searchable();
    const llm = this.state.models.find((m) => m.category === "llm");
    const degraded = !llm || toneOf(llm.status, "llm") !== "green";
    const q = req.text.toLocaleLowerCase();
    let hits: Corpus[];
    if (req.choice_id) {
      const pending = (sess.pending ?? []).map(byRef).filter((c): c is Corpus => !!c);
      const proj = req.choice_id === "proj:modu" ? "MODU" : "20coin Clash";
      hits = req.choice_id === "all" ? pending : pending.filter((c) => c.project === proj);
    } else if (/recent|latest|son belge/.test(q)) {
      hits = [...pool].sort((a, b) => b.modified_at.localeCompare(a.modified_at)).slice(0, 3);
    } else {
      hits = pool.filter((c) => c.keywords.some((k) => q.includes(k)));
    }

    const projects = [...new Set(hits.map((h) => h.project).filter(Boolean))] as string[];
    let resp: SearchResponse;
    const base = { session_id: sess.session_id, turn_id: uid("t"), degraded_llm: degraded, searched_scope_count: this.activeScopes().length };
    if (!req.choice_id && projects.length > 1 && hits.length > 2) {
      resp = {
        ...base, kind: "clarify", count: hits.length, results: [],
        choices: [
          ...projects.map((p) => ({ id: p === "MODU" ? "proj:modu" : "proj:20coin", label: p, count: hits.filter((h) => h.project === p).length })),
          { id: "all", label: "", count: hits.length },
        ],
      };
    } else if (hits.length === 0) {
      resp = { ...base, kind: "not_found", count: 0, choices: [], results: [] };
    } else {
      resp = { ...base, kind: "results", count: hits.length, choices: [], results: hits.slice(0, 4).map(card) };
    }

    sess.pending = resp.kind === "clarify" ? hits.map((h) => h.result_ref) : undefined;
    sess.turns.push({
      turn_id: resp.turn_id, role: "assistant", at: new Date().toISOString(), text: null,
      kind: resp.kind, count: resp.count, choices: resp.choices, results: resp.results, opened: false,
    });
    sess.result_count += resp.results.length;
    this.commit();
    return resp;
  }

  async preview(ref: string) {
    const c = byRef(ref);
    if (!c) throw err("SOURCE_CHANGED", "result no longer available");
    return { result: card(c), body: c.body };
  }

  async openResult(ref: string, _reveal: boolean) {
    if (!byRef(ref)) throw err("SOURCE_CHANGED", "result no longer available");
    for (const s of this.state.sessions) {
      for (const t of s.turns) {
        if (t.results.some((r) => r.result_ref === ref) && !t.opened) {
          t.opened = true;
          s.opened_count += 1;
        }
      }
    }
    this.commit();
  }

  // ------------------------------------------------------------ history

  async listHistory(): Promise<HistorySession[]> {
    return this.state.sessions
      .map(({ turns: _t, pending: _p, ...h }) => h)
      .sort((a, b) => b.started_at.localeCompare(a.started_at));
  }

  async sessionTurns(id: string) {
    return structuredClone(this.state.sessions.find((s) => s.session_id === id)?.turns ?? []);
  }

  async deleteSession(id: string) {
    this.state.sessions = this.state.sessions.filter((s) => s.session_id !== id);
    this.commit();
  }

  async clearHistory() {
    this.state.sessions = [];
    this.commit();
  }
}
