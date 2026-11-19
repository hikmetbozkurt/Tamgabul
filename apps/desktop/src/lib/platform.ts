/** OS-aware UI helpers (single source for Windows / macOS differences). */

export function isMacOs(os: string): boolean {
  return os === "macos";
}

export function modKeyLabel(os: string): "Cmd" | "Ctrl" {
  return isMacOs(os) ? "Cmd" : "Ctrl";
}

export function metaKeyLabel(os: string): "Cmd" | "Win" {
  return isMacOs(os) ? "Cmd" : "Win";
}

export function pathPlaceholder(kind: "folder" | "model", os: string): string {
  if (isMacOs(os)) {
    return kind === "folder" ? "~/Documents" : "~/Models/…";
  }
  return kind === "folder" ? "C:\\Projects" : "C:\\AI\\models\\…";
}

function normalizePath(p: string): { norm: string; windowsDrive: boolean } {
  const windowsDrive = /^[a-zA-Z]:/.test(p);
  const windowsPath = windowsDrive || /^\\\\/.test(p);
  let norm = p.replace(/\\/g, "/");
  if (windowsDrive) {
    norm = norm.length > 3 ? norm.replace(/\/+$/, "") : `${norm.slice(0, 2)}/`;
  } else {
    norm = norm.length > 1 ? norm.replace(/\/+$/, "") : norm;
  }
  if (windowsPath) {
    norm = norm.toLowerCase();
  }
  return { norm, windowsDrive };
}

/** True when `child` is strictly inside `parent` (handles `/` and `\`). */
export function isChildPath(child: string, parent: string): boolean {
  if (!child || !parent) return false;
  const c = normalizePath(child);
  const p = normalizePath(parent);
  if (c.norm === p.norm) return false;
  if (p.norm === "/") return c.norm.startsWith("/") && c.norm.length > 1;
  if (p.windowsDrive && p.norm.endsWith("/")) {
    return c.norm.startsWith(p.norm) && c.norm.length > p.norm.length;
  }
  return c.norm.startsWith(`${p.norm}/`);
}
