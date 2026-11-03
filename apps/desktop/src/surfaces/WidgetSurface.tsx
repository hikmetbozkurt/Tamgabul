import { useEffect, useState } from "react";
import { Icon, Logo } from "../ui/icons";
import { useI18n } from "../i18n";
import { useResource } from "../state/data";
import { useSettings } from "../state/settings";
import { hideSelf, openApp } from "../ipc/bridge";
import { relativeTime } from "../lib/format";
import { SearchBar } from "../features/search/SearchBar";
import { CompactResult } from "../features/search/ResultCard";
import { useSearch } from "../features/search/useSearch";
import { useResultActions } from "../features/search/useResultActions";
import { VoiceView } from "../features/voice/VoiceOverlay";

/** Compact widget (W1–W3): bottom-right on Windows, top-center on macOS. */
export function WidgetSurface() {
  const { t, lang } = useI18n();
  const health = useResource((a) => a.health());
  const history = useResource((a) => a.listHistory().then((h) => h.slice(0, 3)).catch(() => []));
  const s = useSearch();
  const { settings } = useSettings();
  const actions = useResultActions();
  const [voice, setVoice] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !voice && !document.querySelector(".scrim")) hideSelf();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [voice]);

  const h = health.data;
  const emb = h?.models.find((m) => m.category === "embedding");
  const needsSetup = emb?.tone === "red";
  const enabled = !!h?.semantic_search_enabled;
  const mic = h?.models.find((m) => m.category === "stt")?.tone === "green";
  const last = [...s.turns].reverse().find((x) => x.role === "assistant");

  const body = () => {
    if (needsSetup) {
      return (
        <>
          <div className="card err w-setup">
            <span className="icon-tile md err"><Icon name="layers" size={20} stroke={1.6} /></span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 15, fontWeight: 700 }}>EmbeddingGemma 2</div>
              <div className="status err" style={{ marginTop: 2 }}><span className="dot dot-err" />{t("welcome.requiredMissing")}</div>
            </div>
            <button type="button" className="btn btn-xs btn-primary" onClick={() => openApp("welcome")}>{t("models.add")}</button>
          </div>
          <div className="panel rows" style={{ padding: "0 14px" }}>
            <div className="list-row" style={{ padding: "12px 0", fontSize: 14.5 }}><span className="dot dot-neutral" /><span style={{ flex: 1 }}>LLM · STT · TTS</span><span className="small muted">{t("status.OPTIONAL")}</span></div>
            <div className="list-row" style={{ padding: "12px 0", fontSize: 14.5 }}><span className="dot dot-neutral" /><span style={{ flex: 1 }}>{t("welcome.folderTitle")}</span><button type="button" className="btn-link" onClick={() => openApp("folders")}>{t("folders.add")}</button></div>
            <div className="list-row" style={{ padding: "12px 0", fontSize: 14.5 }}><span className="dot dot-ok" /><span style={{ flex: 1 }}>{t("welcome.hotkeyTitle")}</span><span className="small muted">{settings.hotkey.replace(/\+/g, " + ")}</span></div>
          </div>
        </>
      );
    }
    if (s.pending !== null) {
      return <div className="typing" role="status" aria-label={t("search.searching")}><span /><span /><span /></div>;
    }
    if (s.error) {
      return <div className="w-bubble notfound" role="alert"><strong>{t(`error.${s.error.code}.title`)}</strong><span className="muted">{t(`error.${s.error.code}.body`)}</span></div>;
    }
    if (last) {
      if (last.kind === "clarify") {
        return (
          <div className="w-bubble">
            <span>{last.question ?? t("search.clarifyShort")}</span>
            <div className="choices" style={{ gap: 8 }}>
              {last.choices.map((c) => (
                <button key={c.id} type="button" className="chip chip-choice" onClick={() => s.submit(c.id === "all" ? t("search.showAll") : c.label, c.id)}>
                  {c.id === "all" ? t("search.showAll") : c.label}
                </button>
              ))}
            </div>
          </div>
        );
      }
      if (last.kind === "not_found") {
        return (
          <div className="w-bubble notfound">
            <span><strong>{t("search.notFound")}</strong> <span className="muted">{t("search.notFoundShort")}</span></span>
          </div>
        );
      }
      return (
        <>
          {last.answer && <div className="w-bubble direct-answer"><strong>{last.answer.text}</strong></div>}
          {last.results.slice(0, 3).map((r, i) => <CompactResult key={r.result_ref} r={r} primary={i === 0} onOpen={actions.onOpen} />)}
          {(last.count ?? 0) > 3 && (
            <button type="button" className="btn-link" style={{ alignSelf: "center" }} onClick={() => openApp("search")}>{t("widget.seeAll", { n: last.count ?? 0 })}</button>
          )}
        </>
      );
    }
    if (history.data?.length) {
      return (
        <div>
          <div className="section-label" style={{ padding: "4px 12px 6px" }}>{t("search.recent")}</div>
          {history.data.map((x) => (
            <button key={x.session_id} type="button" className="w-recent" onClick={() => s.open(x.session_id)}>
              <Icon name="clock" size={16} color="var(--muted)" />
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{x.title}</span>
              <span className="small muted">{relativeTime(x.started_at, lang)}</span>
            </button>
          ))}
        </div>
      );
    }
    return <p className="w-hint">{t("widget.hint")}</p>;
  };

  return (
    <div className="widget">
      <div className="w-head" data-tauri-drag-region>
        <div className="w-brand" data-tauri-drag-region><Logo size={20} />Tamgabul</div>
        <div className="btns">
          {last && !voice && (
            <button type="button" className="w-icon" aria-label={t("search.new")} title={t("search.new")} onClick={s.reset}>
              <Icon name="plus" size={16} stroke={1.8} />
            </button>
          )}
          <button type="button" className="w-icon" aria-label={t("widget.expand")} title={t("widget.expand")} onClick={() => openApp(needsSetup ? "welcome" : "search")}>
            <Icon name="expand" size={16} stroke={1.8} />
          </button>
          <button type="button" className="w-icon" aria-label={t("window.hide")} title={t("window.hide")} onClick={() => hideSelf()}>
            <Icon name="close" size={16} stroke={1.8} />
          </button>
        </div>
      </div>

      {voice ? (
        <VoiceView onCancel={() => setVoice(false)} onSubmit={(q) => { setVoice(false); s.submit(q); }} />
      ) : (
        <>
          <div className="w-search">
            <SearchBar
              variant="widget"
              disabled={!enabled}
              disabledText={t("search.disabledShort")}
              micAvailable={mic}
              autoFocus
              onSubmit={(q) => s.submit(q)}
              onMic={() => setVoice(true)}
            />
          </div>
          <div className="w-body">{body()}</div>
          <div className="w-foot">
            <button type="button" className="w-link" onClick={() => openApp("memory")}><Icon name="clock" size={16} />{t("widget.history")}</button>
            <span className={`status${needsSetup ? " err" : ""}`}>
              <span className={`dot dot-${needsSetup ? "err" : enabled ? "ok" : "warn"}`} />
              {needsSetup ? t("health.setupNeeded") : enabled ? t("widget.ready") : t("widget.notReady")}
            </span>
            {needsSetup ? (
              <button type="button" className="w-link" onClick={() => openApp("welcome")}>{t("widget.openSetup")}</button>
            ) : (
              <button type="button" className="w-icon" aria-label={t("nav.settings")} title={t("nav.settings")} onClick={() => openApp("settings")}>
                <Icon name="settings" size={17} />
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
