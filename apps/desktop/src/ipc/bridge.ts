// Thin layer over Tauri so the same UI runs in the desktop webview and in a
// plain browser (vite dev, tests, screenshots). Browser = no Rust core.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { IpcError } from "../bindings/IpcError";

export type Surface = "widget" | "app" | "gallery";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function currentSurface(): Surface {
  const s = new URLSearchParams(window.location.search).get("surface");
  return s === "widget" || s === "gallery" ? s : "app";
}

export function isIpcError(e: unknown): e is IpcError {
  return typeof e === "object" && e !== null && "code" in e && "detail" in e;
}

export function toIpcError(e: unknown): IpcError {
  return isIpcError(e) ? e : { code: "INTERNAL", detail: String(e) };
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toIpcError(e);
  }
}

export async function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<T>(event, (e) => cb(e.payload));
}

// ---- window actions (docs/24 D1, D5) ----

/** Open the expanded app (hides the widget). In a browser: switch surface. */
export async function openApp(route?: string): Promise<void> {
  if (isTauri()) return call("show_app", { route: route ?? null });
  window.location.href = `?surface=app#/${route ?? "search"}`;
}

export async function openWidget(): Promise<void> {
  if (isTauri()) return call("show_widget");
  window.location.href = "?surface=widget";
}

export async function hideSelf(): Promise<void> {
  if (!isTauri()) return;
  return call("hide_window", { label: currentSurface() });
}

export async function toggleMaximize(): Promise<void> {
  if (!isTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().toggleMaximize();
}

export async function pickPath(directory: boolean, title: string): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory, multiple: false, title });
  return typeof r === "string" ? r : null;
}
