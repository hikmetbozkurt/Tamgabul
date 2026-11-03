import { useEffect, useState } from "react";
import type { ModelCategory } from "../../bindings/ModelCategory";
import type { ModelEntry } from "../../bindings/ModelEntry";
import type { TtlPolicy } from "../../bindings/TtlPolicy";
import { Icon } from "../../ui/icons";
import { ErrorState, Pill, Skeleton } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData, useResource } from "../../state/data";
import { useToast } from "../../state/toast";
import { relativeTime } from "../../lib/format";
import { CATEGORIES, isLoaded, toneOf } from "../../lib/health";
import { CATEGORY_ICON, statusKey } from "../../lib/labels";
import { toIpcError } from "../../ipc/bridge";
import { clearHashFlags, hashFlag } from "../../shell/router";
import { AddModelDialog } from "./AddModelDialog";

const TTLS: TtlPolicy[] = ["immediately", "minutes5", "minutes10", "minutes30", "hours1", "never"];

export function ModelsPage() {
  const { t, lang } = useI18n();
  const { api } = useData();
  const toast = useToast();
  const models = useResource((a) => a.listModels());
  const [adding, setAdding] = useState<ModelCategory | null>(null);

  useEffect(() => {
    if (hashFlag("add")) {
      const missingEmb = !models.data?.some((m) => m.category === "embedding");
      setAdding(missingEmb ? "embedding" : "llm");
      clearHashFlags();
    }
  }, [models.data]);

  const act = (p: Promise<unknown>) => p.catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));
  const slot = (c: ModelCategory) => models.data?.find((m) => m.category === c);
  const emb = slot("embedding");

  const ttlSelect = (m: ModelEntry, id: string) => (
    <select id={id} className="select sm" value={m.ttl} onChange={(e) => act(api.setTtl(m.model_id, e.target.value as TtlPolicy))}>
      {TTLS.map((x) => <option key={x} value={x}>{t(`ttl.${x}`)}</option>)}
    </select>
  );

  const actions = (m: ModelEntry | undefined, c: ModelCategory, big?: boolean) => {
    const cls = big ? "btn btn-sm" : "btn btn-sm";
    if (!m) {
      return <button type="button" className="btn btn-sm btn-primary" onClick={() => setAdding(c)}><Icon name="plus" size={16} stroke={1.8} />{t("models.add")}</button>;
    }
    const busy = ["LOADING", "UNLOADING", "VALIDATING"].includes(m.status);
    if (busy) return <span className="status"><span className="spinner" />{t(statusKey(m.status, m.required))}</span>;
    if (m.status === "INCOMPATIBLE" || m.status === "ERROR" || m.status === "MISSING") {
      return (
        <>
          {m.status === "ERROR" && (
            <button type="button" className={`${cls} btn-primary`} onClick={() => act(api.loadModel(m.model_id))}>{t("common.retry")}</button>
          )}
          <button type="button" className={cls} onClick={() => setAdding(c)}>{t("models.replace")}</button>
          <button type="button" className={`${cls} btn-danger`} onClick={() => act(api.removeModel(m.model_id))}>{t("models.remove")}</button>
        </>
      );
    }
    return (
      <>
        {isLoaded(m.status) ? (
          <button type="button" className={cls} onClick={() => act(api.unloadModel(m.model_id))}>{t("models.unload")}</button>
        ) : (
          <button type="button" className={`${cls} btn-primary`} onClick={() => act(api.loadModel(m.model_id))}>{t("models.load")}</button>
        )}
        <button type="button" className={cls} onClick={() => setAdding(c)}>{t("models.replace")}…</button>
      </>
    );
  };

  return (
    <div className="page-scroll">
      <div className="page">
        <div className="page-head">
          <div>
            <h1 className="page-title">{t("nav.models")}</h1>
            <p className="page-sub">{t("models.sub")}</p>
          </div>
          <div className="page-actions">
            <button type="button" className="btn" disabled={!models.data?.some((m) => isLoaded(m.status))} onClick={() => act(api.unloadAll())}>{t("models.unloadAll")}</button>
            <button type="button" className="btn btn-primary" onClick={() => setAdding(emb ? "llm" : "embedding")}><Icon name="plus" size={16} stroke={1.8} />{t("models.add")}</button>
          </div>
        </div>

        {models.error && models.error.code !== "NOT_IMPLEMENTED" ? (
          <div className="card"><ErrorState error={models.error} onRetry={models.reload} /></div>
        ) : models.loading ? (
          <><Skeleton h={220} r={14} /><div className="model-grid"><Skeleton h={240} r={14} /><Skeleton h={240} r={14} /><Skeleton h={240} r={14} /></div></>
        ) : (
          <>
            <section className={`card model-main${!emb || toneOf(emb.status, "embedding") === "red" ? " err" : ""}`}>
              <div className="model-head">
                <span className={`icon-tile lg ${emb && emb.tone !== "red" ? "file" : "err"}`}><Icon name="layers" size={24} stroke={1.6} /></span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                    <h2>EmbeddingGemma 2</h2>
                    <span className="badge">{t("models.requiredBadge")}</span>
                  </div>
                  <div className="small muted" style={{ marginTop: 4, fontSize: 14 }}>
                    {emb ? `${t("category.embedding")} · ${emb.source_label ?? "—"} · ${t("models.textImage")}` : t("models.embMissing")}
                  </div>
                </div>
                <Pill tone={emb?.tone ?? "red"} large>{t(statusKey(emb?.status ?? "MISSING", true))}</Pill>
              </div>
              {emb && (emb.error_key || emb.error_code) && (
                <div className="note err"><Icon name="warn" size={18} /><span>{t(emb.error_key ?? `error.${emb.error_code}.body`)}</span></div>
              )}
              {emb && (
                <div className="facts">
                  <div><div className="k">{t("models.runtime")}</div><div className="v">{emb.runtime ?? t("models.notVerified")}</div></div>
                  <div><div className="k">{t("models.dimension")}</div><div className="v">{emb.dimension ?? "—"}</div></div>
                  <div><div className="k">{t("models.memory")}</div><div className="v">{emb.memory_mb != null ? `${emb.memory_mb} MB` : t("common.notMeasured")}</div></div>
                  <div><div className="k">{t("models.validated")}</div><div className="v">{emb.validated_at ? relativeTime(emb.validated_at, lang) : "—"}</div></div>
                  <div><label className="k" htmlFor="ttl-emb">{t("models.ttl")}</label><div style={{ marginTop: 4 }}>{ttlSelect(emb, "ttl-emb")}</div></div>
                </div>
              )}
              <div className="model-actions">
                {actions(emb, "embedding", true)}
                <span className="small muted" style={{ marginLeft: "auto" }}>{t("models.swapNote")}</span>
              </div>
            </section>

            <div className="model-grid">
              {CATEGORIES.filter((c) => c !== "embedding").map((c) => {
                const m = slot(c);
                const tone = m?.tone ?? toneOf("MISSING", c);
                const bad = tone === "red";
                return (
                  <section key={c} className={`card model-card${bad ? " err" : ""}`}>
                    <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                      <span className="icon-tile md"><Icon name={CATEGORY_ICON[c]} size={20} stroke={1.6} /></span>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <h2>{t(`category.${c}`)}</h2>
                        <div className="small muted">{m?.display_name ?? t("panel.notInstalled")}</div>
                      </div>
                    </div>
                    <Pill tone={tone}>{t(statusKey(m?.status ?? "MISSING", false))}</Pill>
                    <div className={`desc${bad ? " err" : ""}`}>
                      {m?.error_key ? t(m.error_key) : t(`models.desc.${c}`)}
                      {m && !bad && <> {t("models.ttlLine", { ttl: t(`ttl.${m.ttl}`) })}</>}
                    </div>
                    <div className="model-actions">
                      {actions(m, c)}
                      {m && !bad && (
                        <span style={{ marginLeft: "auto" }}>
                          <label htmlFor={`ttl-${c}`} className="sr-only">{t("models.ttl")}</label>
                          {ttlSelect(m, `ttl-${c}`)}
                        </span>
                      )}
                    </div>
                  </section>
                );
              })}
            </div>

            <div className="legend">
              {(["green", "amber", "red", "neutral"] as const).map((tone) => (
                <span key={tone} className="status"><span className={`dot dot-${{ green: "ok", amber: "warn", red: "err", neutral: "neutral" }[tone]}`} />{t(`legend.${tone}`)}</span>
              ))}
            </div>
            {models.error?.code === "NOT_IMPLEMENTED" && <div className="note"><Icon name="info" size={18} />{t("models.p4Note")}</div>}
          </>
        )}
      </div>
      {adding && <AddModelDialog initial={adding} onClose={() => setAdding(null)} />}
    </div>
  );
}
