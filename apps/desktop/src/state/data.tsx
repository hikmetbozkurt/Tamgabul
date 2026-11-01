import {
  createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode,
} from "react";
import type { Api } from "../ipc/api";
import { MockApi } from "../ipc/mock";
import { RealApi } from "../ipc/real";
import { on, toIpcError } from "../ipc/bridge";
import type { IpcError } from "../bindings/IpcError";
import { useSettings } from "./settings";

type Ctx = { api: Api; version: number; refresh: () => void };
const DataCtx = createContext<Ctx | null>(null);

/** Picks the MOCK or real backend from settings and re-renders on changes. */
export function DataProvider({ children }: { children: ReactNode }) {
  const { settings } = useSettings();
  const api = useMemo<Api>(
    () => (settings.demo_data ? new MockApi(settings.demo_scenario) : new RealApi()),
    [settings.demo_data, settings.demo_scenario],
  );
  const [version, setVersion] = useState(0);
  const refresh = useCallback(() => setVersion((v) => v + 1), []);

  useEffect(() => {
    const unsub = api.subscribe(refresh);
    refresh();
    return () => {
      unsub();
      (api as { dispose?: () => void }).dispose?.();
    };
  }, [api, refresh]);

  // Tray menu actions (docs/09): handled by whichever backend is active.
  useEffect(() => {
    const p = on<string>("tray_action", (action) => {
      if (action === "unload_models") api.unloadAll().catch(() => {});
      refresh();
    });
    return () => { p.then((u) => u()); };
  }, [api, refresh]);

  return <DataCtx.Provider value={{ api, version, refresh }}>{children}</DataCtx.Provider>;
}

export function useData() {
  const c = useContext(DataCtx);
  if (!c) throw new Error("DataProvider missing");
  return c;
}

export type Resource<T> = { data?: T; error: IpcError | null; loading: boolean; reload: () => void };

/**
 * Loads `fn` and reloads whenever data changes. Keeps the previous value while
 * reloading so lists do not flash; `loading` is true only for the first load.
 */
export function useResource<T>(fn: (api: Api) => Promise<T>, deps: unknown[] = []): Resource<T> {
  const { api, version, refresh } = useData();
  const [state, setState] = useState<{ data?: T; error: IpcError | null; loading: boolean }>({ error: null, loading: true });
  const fnRef = useRef(fn);
  fnRef.current = fn;
  useEffect(() => {
    let alive = true;
    fnRef.current(api).then(
      (data) => alive && setState({ data, error: null, loading: false }),
      (e) => alive && setState((s) => ({ data: s.data, error: toIpcError(e), loading: false })),
    );
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, version, ...deps]);
  return { ...state, reload: refresh };
}
