import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import type { AppSettings } from "../bindings/AppSettings";
import type { SettingsSnapshot } from "../bindings/SettingsSnapshot";
import { call, isTauri, on } from "../ipc/bridge";

// Same defaults as crates/core settings.rs (browser preview has no core).
export const DEFAULT_SETTINGS: AppSettings = {
  language: "en",
  hotkey: "Ctrl+Shift+Space",
  widget_position: "platform_default",
  mic_mode: "push_to_talk",
  launch_at_startup: false,
  history_enabled: true,
  history_retention: "days90",
  indexing_mode: "manual",
  llm_understand: false,
  llm_pick: true,
  llm_caption: true,
};

const LS_KEY = "tamgabul.settings";

function browserSnapshot(): SettingsSnapshot {
  let settings = DEFAULT_SETTINGS;
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) settings = { ...DEFAULT_SETTINGS, ...(JSON.parse(raw) as Partial<AppSettings>) };
  } catch { /* defaults */ }
  return { settings, hotkey_active: true, os: "browser" };
}

type Ctx = {
  snapshot: SettingsSnapshot;
  settings: AppSettings;
  /** Persists through the core; rejects with an IpcError (e.g. HOTKEY_CONFLICT). */
  update: (patch: Partial<AppSettings>) => Promise<void>;
};

const SettingsCtx = createContext<Ctx | null>(null);

export function SettingsProvider({
  children,
  initialSnapshot,
  onUpdate,
}: {
  children: ReactNode;
  initialSnapshot?: SettingsSnapshot;
  onUpdate?: (next: AppSettings) => Promise<SettingsSnapshot>;
}) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(
    initialSnapshot ?? (isTauri() ? null : browserSnapshot()),
  );

  useEffect(() => {
    if (initialSnapshot) return;
    if (isTauri()) {
      call<SettingsSnapshot>("get_settings").then(setSnapshot).catch(() => setSnapshot(browserSnapshot()));
      const p = on<SettingsSnapshot>("settings_changed", setSnapshot);
      return () => { p.then((u) => u()); };
    }
    const onStorage = (e: StorageEvent) => e.key === LS_KEY && setSnapshot(browserSnapshot());
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [initialSnapshot]);

  const update = useCallback(
    async (patch: Partial<AppSettings>) => {
      if (!snapshot) return;
      const next = { ...snapshot.settings, ...patch };
      if (onUpdate) {
        setSnapshot(await onUpdate(next));
      } else if (isTauri()) {
        setSnapshot(await call<SettingsSnapshot>("set_settings", { next }));
      } else {
        localStorage.setItem(LS_KEY, JSON.stringify(next));
        setSnapshot({ ...snapshot, settings: next });
      }
    },
    [snapshot, onUpdate],
  );

  if (!snapshot) return null;
  return (
    <SettingsCtx.Provider value={{ snapshot, settings: snapshot.settings, update }}>{children}</SettingsCtx.Provider>
  );
}

export function useSettings() {
  const c = useContext(SettingsCtx);
  if (!c) throw new Error("SettingsProvider missing");
  return c;
}
