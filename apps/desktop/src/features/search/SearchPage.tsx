import { useState } from "react";
import { Icon } from "../../ui/icons";
import { useI18n } from "../../i18n";
import { useResource } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { relativeTime } from "../../lib/format";
import { StatusPanel } from "../../shell/StatusPanel";
import { navigate } from "../../shell/router";
import { SearchBar } from "./SearchBar";
import { Thread } from "./Thread";
import { PreviewDrawer } from "./PreviewDrawer";
import { useSearch } from "./useSearch";
import { useResultActions } from "./useResultActions";
import { VoiceOverlay } from "../voice/VoiceOverlay";

const SUGGESTIONS = ["search.suggest1", "search.suggest2", "search.suggest3"];

export function SearchPage() {
  const { t, lang } = useI18n();
  const toast = useToast();
  const s = useSearch();
  const { settings } = useSettings();
  const actions = useResultActions();
  const health = useResource((a) => a.health());
  const history = useResource((a) => a.listHistory().then((h) => h.slice(0, 3)));
  const [preview, setPreview] = useState<string | null>(null);
  const [voice, setVoice] = useState(false);

  const enabled = !!health.data?.semantic_search_enabled;
  const mic = health.data?.models.find((m) => m.category === "stt")?.tone === "green";
  const inConvo = !!s.sessionId && (s.turns.length > 0 || s.pending !== null);

  const onSuggest = (kind: "filenames" | "ocr" | "folder") => {
    if (kind === "folder") navigate("folders");
    else toast(t("search.modeLater"));
  };

  const bar = (variant: "hero" | "follow") => (
    <SearchBar
      key={variant}
      variant={variant}
      disabled={!enabled}
      disabledText={t("search.disabled")}
      micAvailable={mic}
      autoFocus
      onSubmit={(q) => s.submit(q)}
      onMic={() => setVoice(true)}
    />
  );

  return (
    <>
      <div className="page-scroll">
        {inConvo ? (
          <div className="search-page convo">
            <div className="search-wrap">
              <div className="convo-head">
                {s.degraded ? (
                  <span className="banner"><span className="dot dot-warn" />{t("search.degraded")}</span>
                ) : <span />}
                <button type="button" className="btn btn-sm btn-ghost" onClick={s.reset}>
                  <Icon name="plus" size={16} stroke={1.8} />{t("search.new")}
                </button>
              </div>
              <Thread
                turns={s.turns}
                pending={s.pending}
                error={s.error}
                onChoice={(id, label) => s.submit(label, id)}
                onOpen={actions.onOpen}
                onReveal={actions.onReveal}
                onPreview={setPreview}
                onSuggest={onSuggest}
              />
              <div style={{ position: "sticky", bottom: 24 }}>{bar("follow")}</div>
            </div>
          </div>
        ) : (
          <div className="search-page home">
            <div className="search-wrap">
              <h1 className="hero-title">Tamgabul</h1>
              <p className="hero-sub">{t("search.tagline")}</p>
              {bar("hero")}
              {!enabled && health.data && (
                <div className="note err" style={{ marginTop: 16 }}>
                  <Icon name="warn" size={18} />
                  <span>{t("search.needsEmbedding")} <a href="#/models" style={{ color: "inherit", fontWeight: 600 }}>{t("search.openModels")}</a></span>
                </div>
              )}
              {s.error && <div className="note err" style={{ marginTop: 16 }} role="alert">{t(`error.${s.error.code}.title`)}</div>}
              <div className="suggestions">
                {SUGGESTIONS.map((k) => (
                  <button key={k} type="button" className="chip" disabled={!enabled} onClick={() => s.submit(t(k))}>{t(k)}</button>
                ))}
              </div>
              {!!history.data?.length && (
                <section className="recent">
                  <div className="recent-head">
                    <h2>{t("search.recent")}</h2>
                    <a href="#/memory" className="btn-link" style={{ textDecoration: "none" }}>{t("search.seeAll")}</a>
                  </div>
                  <div className="panel rows" style={{ padding: "0 16px" }}>
                    {history.data.map((h) => (
                      <button key={h.session_id} type="button" className="recent-row" onClick={() => s.open(h.session_id)}>
                        <span className="icon-tile"><Icon name="clock" size={17} /></span>
                        <span className="q">{h.title}</span>
                        <span className="small muted">
                          {t("memory.results", { n: h.result_count })} · {relativeTime(h.started_at, lang)}
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              )}
              <p className="hint">{t("search.hotkeyHint")} <kbd>{settings.hotkey.replace(/\+/g, " + ")}</kbd></p>
            </div>
          </div>
        )}
      </div>
      <StatusPanel />
      {preview && (
        <PreviewDrawer resultRef={preview} onClose={() => setPreview(null)} onOpen={actions.onOpen} onReveal={actions.onReveal} />
      )}
      {voice && <VoiceOverlay onClose={() => setVoice(false)} onSubmit={(q) => { setVoice(false); s.submit(q); }} />}
    </>
  );
}
