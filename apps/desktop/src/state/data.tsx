import {
  createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode,
} from "react";
import type { Api } from "../ipc/api";
import { CoreApi } from "../ipc/core";
import { on, toIpcError } from "../ipc/bridge";
import type { IpcError } from "../bindings/IpcError";

type Ctx = { api: Api; version: number; refresh: () => void };
const DataCtx = createContext<Ctx | null>(null);

/** Connects the UI to the Rust core and re-renders when data changes. */
export function DataProvider({ children }: { children: ReactNode }) {
  const api = useMemo<Api>(() => new CoreApi(), []);
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

  // Tray actions and model lifecycle changes are done by the Rust core;
  // the UI only re-reads state.
  useEffect(() => {
    const subs = ["tray_action", "model_status", "scope_status"].map((ev) => on(ev, refresh));
    return () => { subs.forEach((p) => p.then((u) => u())); };
  }, [refresh]);

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
