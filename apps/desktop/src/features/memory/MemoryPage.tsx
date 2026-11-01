import { useMemo, useState } from "react";
import { Icon } from "../../ui/icons";
import { Dialog, EmptyState, ErrorState, Skeleton, Toggle } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData, useResource } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { clockTime, dayBucket, relativeTime, weekday } from "../../lib/format";
import { FILE_ICON } from "../../lib/labels";
import { currentSession } from "../../ipc/api";
import { toIpcError } from "../../ipc/bridge";
import { navigate } from "../../shell/router";
import { useResultActions } from "../search/useResultActions";

const BUCKETS = ["today", "yesterday", "week", "older"] as const;

export function MemoryPage() {
  const { t, lang } = useI18n();
  const { api } = useData();
  const { settings, update } = useSettings();
  const toast = useToast();
  const actions = useResultActions();
  const sessions = useResource((a) => a.listHistory());
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<"one" | "all" | null>(null);

  const list = useMemo(
    () => (sessions.data ?? []).filter((s) => s.title.toLocaleLowerCase(lang).includes(filter.toLocaleLowerCase(lang))),
    [sessions.data, filter, lang],
  );
  const current = list.find((s) => s.session_id === selected) ?? list[0];
  const turns = useResource((a) => (current ? a.sessionTurns(current.session_id) : Promise.resolve([])), [current?.session_id]);

  const meta = (s: (typeof list)[number]) => {
    const parts = [s.result_count ? t("memory.results", { n: s.result_count }) : t("memory.noResults")];
    if (s.opened_count) parts.push(t("memory.opened", { n: s.opened_count }));
    const b = dayBucket(s.started_at);
    parts.push(b === "today" || b === "yesterday" ? clockTime(s.started_at, lang) : b === "week" ? weekday(s.started_at, lang) : relativeTime(s.started_at, lang));
    return parts.join(" · ");
  };

  const doDelete = async () => {
    try {
      if (confirm === "all") await api.clearHistory();
      else if (current) await api.deleteSession(current.session_id);
      if (confirm === "all" || current?.session_id === currentSession.get()) currentSession.set(null);
      toast(t(confirm === "all" ? "memory.clearedToast" : "memory.deletedToast"));
      setSelected(null);
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
    }
    setConfirm(null);
  };

  return (
    <div className="page-scroll">
      <div className="page">
        <div className="page-head">
          <div>
            <h1 className="page-title">{t("nav.memory")}</h1>
            <p className="page-sub">{t("memory.sub")}</p>
          </div>
          <div className="page-actions">
            <div className="searchbar" style={{ height: 44, width: 260, padding: "0 16px", boxShadow: "none", gap: 10 }}>
              <Icon name="search" size={17} stroke={1.8} color="var(--muted)" />
              <label htmlFor="mem-filter" className="sr-only">{t("memory.filter")}</label>
              <input id="mem-filter" placeholder={t("memory.filter")} value={filter} onChange={(e) => setFilter(e.target.value)} style={{ fontSize: 15 }} />
            </div>
            <span className="setting-row" style={{ gap: 10 }}>
              <Toggle checked={settings.history_enabled} label={t("memory.keep")} onChange={(v) => update({ history_enabled: v })} />
              {t("memory.keep")}
            </span>
            <button type="button" className="btn btn-danger" disabled={!sessions.data?.length} onClick={() => setConfirm("all")}>{t("memory.clearAll")}</button>
          </div>
        </div>

        {sessions.error ? (
          <div className="card"><ErrorState error={sessions.error} onRetry={sessions.reload} /></div>
        ) : sessions.loading ? (
          <div className="memory-cols"><div className="sessions"><Skeleton h={64} /><Skeleton h={64} /><Skeleton h={64} /></div></div>
        ) : !list.length ? (
          <div className="card">
            <EmptyState icon={<span className="icon-tile md"><Icon name="memory" size={20} /></span>} title={filter ? t("memory.noMatch") : t("memory.empty")}>
              {!filter && <span>{t("memory.emptyBody")}</span>}
            </EmptyState>
          </div>
        ) : (
          <div className="memory-cols">
            <section className="sessions" aria-label={t("memory.sessions")}>
              {BUCKETS.map((b) => {
                const items = list.filter((s) => dayBucket(s.started_at) === b);
                if (!items.length) return null;
                return (
                  <div key={b} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                    <div className="section-label" style={{ padding: "8px 4px 0" }}>{t(`memory.bucket.${b}`)}</div>
                    {items.map((s) => (
                      <button
                        key={s.session_id}
                        type="button"
                        className="session-item"
                        aria-current={s.session_id === current?.session_id}
                        onClick={() => setSelected(s.session_id)}
                      >
                        <span className="t">{s.title}</span>
                        <span className="m">{meta(s)}</span>
                      </button>
                    ))}
                  </div>
                );
              })}
            </section>

            {current && (
              <section className="card session-detail" aria-label={t("memory.detail")}>
                <div className="page-head" style={{ alignItems: "center" }}>
                  <div>
                    <h2 style={{ fontSize: 19, fontWeight: 700 }}>{current.title}</h2>
                    <div className="small muted" style={{ marginTop: 4 }}>
                      {relativeTime(current.started_at, lang)} · {t("memory.steps", { n: turns.data?.length ?? 0 })}
                    </div>
                  </div>
                  <div className="page-actions">
                    <button
                      type="button"
                      className="btn btn-primary"
                      onClick={() => { currentSession.set(current.session_id); navigate("search"); }}
                    >
                      {t("memory.continue")}
                    </button>
                    <button type="button" className="btn btn-icon btn-sm" style={{ color: "var(--err-text)" }} aria-label={t("memory.deleteOne")} title={t("memory.deleteOne")} onClick={() => setConfirm("one")}>
                      <Icon name="trash" size={18} />
                    </button>
                  </div>
                </div>

                <ol className="timeline">
                  {(turns.data ?? []).map((turn) => (
                    <li key={turn.turn_id}>
                      <span className={`node${turn.role === "assistant" ? (turn.results.length ? " res" : " ai") : ""}`} />
                      {turn.role === "user" ? (
                        <>
                          <div className="who">{t("memory.you")} · {clockTime(turn.at, lang)}</div>
                          <div className="what">{turn.text}</div>
                        </>
                      ) : (
                        <>
                          <div className="who">
                            {turn.kind === "clarify" ? t("memory.clarified") : turn.kind === "not_found" ? t("memory.notFound") : t("memory.shown")}
                          </div>
                          {turn.kind === "clarify" && (
                            <div className="what">{t("search.clarify", { n: turn.count ?? 0, projects: turn.choices.length - 1 })}</div>
                          )}
                          {turn.kind === "not_found" && <div className="what muted">{t("search.notFound")}</div>}
                          {turn.results.map((r) => (
                            <div className="mini-result" key={r.result_ref}>
                              <span className="icon-tile file" style={{ width: 44, height: 44 }}><Icon name={FILE_ICON[r.file_kind]} size={22} /></span>
                              <div style={{ flex: 1, minWidth: 0 }}>
                                <div style={{ fontWeight: 600 }}>{r.title}</div>
                                <div className="small muted">{r.display_path}</div>
                              </div>
                              {turn.opened && <span className="opened-badge">{t("result.opened")}</span>}
                              <button type="button" className="btn btn-xs" onClick={() => actions.onOpen(r.result_ref)}>{t("memory.reopen")}</button>
                            </div>
                          ))}
                        </>
                      )}
                    </li>
                  ))}
                </ol>
                <p className="page-foot" style={{ marginTop: "auto" }}>{t("memory.deleteNote")}</p>
              </section>
            )}
          </div>
        )}
      </div>

      {confirm && (
        <Dialog title={t(confirm === "all" ? "memory.confirmAllTitle" : "memory.confirmOneTitle")} onClose={() => setConfirm(null)} small>
          <p style={{ fontSize: 14.5, lineHeight: 1.5, color: "var(--ink-2)" }}>{t("memory.deleteNote")}</p>
          <div className="dialog-actions">
            <button type="button" className="btn" onClick={() => setConfirm(null)}>{t("common.cancel")}</button>
            <button type="button" className="btn btn-danger-solid" onClick={doDelete}>{t("common.delete")}</button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
