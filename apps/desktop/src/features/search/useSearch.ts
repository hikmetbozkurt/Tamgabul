import { useCallback, useEffect, useState } from "react";
import { currentSession } from "../../ipc/api";
import { toIpcError } from "../../ipc/bridge";
import { useData, useResource } from "../../state/data";
import type { IpcError } from "../../bindings/IpcError";

/** Conversation state shared by the widget and the app (same session id). */
export function useSearch() {
  const { api } = useData();
  const [sessionId, setSessionId] = useState<string | null>(currentSession.get);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [degraded, setDegraded] = useState(false);

  // Follow session changes made in the other window.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === "tamgabul.currentSession") setSessionId(e.newValue);
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const turns = useResource((a) => (sessionId ? a.sessionTurns(sessionId) : Promise.resolve([])), [sessionId]);

  const submit = useCallback(
    async (text: string, choiceId: string | null = null) => {
      const q = text.trim();
      if (!q && !choiceId) return;
      setPending(q);
      setError(null);
      try {
        const r = await api.search({
          session_id: sessionId,
          text: q,
          choice_id: choiceId,
          // Minutes east of UTC, so "today" and "last week" follow the user's clock.
          utc_offset_min: -new Date().getTimezoneOffset(),
        });
        setDegraded(r.degraded_llm);
        if (r.session_id !== sessionId) {
          currentSession.set(r.session_id);
          setSessionId(r.session_id);
        }
      } catch (e) {
        setError(toIpcError(e));
      } finally {
        setPending(null);
      }
    },
    [api, sessionId],
  );

  const reset = useCallback(() => {
    currentSession.set(null);
    setSessionId(null);
    setError(null);
    setDegraded(false);
  }, []);

  const open = useCallback((id: string) => {
    currentSession.set(id);
    setSessionId(id);
    setError(null);
  }, []);

  return { sessionId, turns: turns.data ?? [], pending, error, degraded, submit, reset, open };
}
