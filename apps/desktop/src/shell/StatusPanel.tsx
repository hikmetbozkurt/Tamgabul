import { useEffect, useState } from "react";
import { FolderIcon, Icon } from "../ui/icons";
import { Status, Toggle } from "../ui/primitives";
import { useT } from "../i18n";
import { useData, useResource } from "../state/data";
import { useToast } from "../state/toast";
import { CATEGORY_ICON, statusKey } from "../lib/labels";
import { CATEGORIES, toneOf } from "../lib/health";
import { toIpcError } from "../ipc/bridge";

const LS = "tamgabul.panel";
type Open = { system: boolean; folders: boolean };

function initialOpen(): Open {
  try {
    const raw = localStorage.getItem(LS);
    if (raw) return JSON.parse(raw) as Open;
  } catch { /* default */ }
  // docs/24 D7: narrow windows start collapsed.
  const wide = window.matchMedia?.("(min-width: 1200px)").matches ?? true;
  return { system: wide, folders: wide };
}

/** Right column on Search: System Status + Folders, each collapsible. */
export function StatusPanel() {
  const t = useT();
  const toast = useToast();
  const { api } = useData();
  const models = useResource((a) => a.listModels());
  const scopes = useResource((a) => a.listScopes());
  const health = useResource((a) => a.health());
  const [open, setOpen] = useState<Open>(initialOpen);
  useEffect(() => {
    try { localStorage.setItem(LS, JSON.stringify(open)); } catch { /* ignore */ }
  }, [open]);

  const rows = CATEGORIES.map((c) => {
    const m = models.data?.find((x) => x.category === c);
    const status = m?.status ?? "MISSING";
    return { c, m, status, tone: m?.tone ?? toneOf(status, c) };
  });
  const loaded = rows.filter((r) => r.tone === "green").length;
  const visibleScopes = (scopes.data ?? []).filter((s) => s.status !== "revoking");

  const toggleWatch = (id: string, on: boolean) =>
    api.setWatching(id, on).catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));

  return (
    <aside className="aside" aria-label={t("panel.label")}>
      {open.system ? (
        <>
          <button type="button" className="aside-head" aria-expanded="true" onClick={() => setOpen({ ...open, system: false })}>
            <Icon name="settings" size={19} />
            <h2>{t("panel.system")}</h2>
            <span className="chev"><Icon name="chevronUp" size={16} stroke={1.8} /></span>
          </button>
          <div className="panel list rows">
            {rows.map(({ c, m, status, tone }) => (
              <div className="list-row" key={c}>
                <span className="icon-tile"><Icon name={CATEGORY_ICON[c]} size={18} stroke={1.6} /></span>
                <div className="grow">
                  <div className="name">{t(`category.${c}`)}</div>
                  <div className="sub">{m?.display_name ?? t("panel.notInstalled")}</div>
                </div>
                <Status tone={tone}>{t(statusKey(status, c === "embedding"))}</Status>
              </div>
            ))}
          </div>
          <a href="#/models?add" className="btn btn-soft"><Icon name="plus" size={16} stroke={1.8} />{t("models.add")}</a>
        </>
      ) : (
        <button type="button" className="aside-collapsed" aria-expanded="false" onClick={() => setOpen({ ...open, system: true })}>
          <Icon name="settings" size={19} />
          <span style={{ flex: 1 }}>
            <span className="t">{t("panel.system")}</span>
            <span className="s">
              <span className={`dot dot-${health.data?.semantic_search_enabled ? "ok" : "err"}`} />
              {health.data?.semantic_search_enabled ? t("panel.searchReady") : t("panel.searchOff")}
              {" · "}{t("panel.loadedCount", { n: loaded })}
            </span>
          </span>
          <Icon name="chevronDown" size={16} stroke={1.8} />
        </button>
      )}

      <div className="aside-sep" />

      {open.folders ? (
        <>
          <button type="button" className="aside-head" aria-expanded="true" onClick={() => setOpen({ ...open, folders: false })}>
            <Icon name="memory" size={19} />
            <h2>{t("panel.folders")}</h2>
            <span className="chev"><Icon name="chevronUp" size={16} stroke={1.8} /></span>
          </button>
          {scopes.error ? (
            <div className="note">{t(`error.${scopes.error.code}.title`)}</div>
          ) : visibleScopes.length === 0 ? (
            <div className="note">{t("panel.noFolders")}</div>
          ) : (
            <div className="panel list rows">
              {visibleScopes.slice(0, 4).map((s) => (
                <div className="list-row" key={s.scope_id}>
                  <FolderIcon tone={s.status === "unreachable" ? "err" : s.watching ? "normal" : "off"} />
                  <div className="grow">
                    <div className="name" style={{ fontWeight: 500, fontSize: s.display_path.length > 18 ? 14 : 15.5 }}>{s.display_path}</div>
                    <div className="sub">
                      {s.status === "unreachable" ? t("folders.status.unreachable") : s.watching ? t("panel.watching") : t("panel.notWatching")}
                    </div>
                  </div>
                  {s.status !== "unreachable" && (
                    <Toggle checked={s.watching} label={t("folders.watchLabel", { path: s.display_path })} onChange={(v) => toggleWatch(s.scope_id, v)} />
                  )}
                </div>
              ))}
            </div>
          )}
          <a href="#/folders?add" className="btn btn-soft"><Icon name="plus" size={16} stroke={1.8} />{t("folders.add")}</a>
        </>
      ) : (
        <button type="button" className="aside-collapsed" aria-expanded="false" onClick={() => setOpen({ ...open, folders: true })}>
          <Icon name="memory" size={19} />
          <span style={{ flex: 1 }}>
            <span className="t">{t("panel.folders")}</span>
            <span className="s">{t("panel.watchingCount", { n: visibleScopes.filter((s) => s.watching).length })}</span>
          </span>
          <Icon name="chevronDown" size={16} stroke={1.8} />
        </button>
      )}
    </aside>
  );
}
