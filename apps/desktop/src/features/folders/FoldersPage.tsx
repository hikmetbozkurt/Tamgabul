import { useEffect, useState } from "react";
import type { ScopeSummary } from "../../bindings/ScopeSummary";
import type { StatusTone } from "../../bindings/StatusTone";
import { FolderIcon, Icon } from "../../ui/icons";
import { EmptyState, ErrorState, Segmented, Skeleton, Status, Toggle } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData, useResource } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { formatMb, formatNumber, relativeTime } from "../../lib/format";
import { toIpcError } from "../../ipc/bridge";
import { clearHashFlags, hashFlag } from "../../shell/router";
import { AddFolderDialog, RevokeDialog } from "./FolderDialogs";

const STATUS_TONE: Record<ScopeSummary["status"], StatusTone> = {
  up_to_date: "green",
  scanning: "amber",
  revoking: "amber",
  paused: "neutral",
  unreachable: "red",
  error: "red",
};

export function FoldersPage() {
  const { t, lang } = useI18n();
  const { api } = useData();
  const { settings, update } = useSettings();
  const toast = useToast();
  const scopes = useResource((a) => a.listScopes());
  const stats = useResource((a) => a.indexStats());
  const [adding, setAdding] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<ScopeSummary | null>(null);

  useEffect(() => {
    if (hashFlag("add")) { setAdding(true); clearHashFlags(); }
  }, []);

  const act = (p: Promise<unknown>) => p.catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));
  const list = scopes.data ?? [];
  const pathOf = (id: string | null) => list.find((s) => s.scope_id === id)?.display_path ?? null;

  const metaLine = (s: ScopeSummary) => {
    const bits = [
      s.status === "unreachable" ? t("folders.unreachableMeta")
        : s.status === "error" && s.error_code ? t(`error.${s.error_code}.title`)
        : s.watching ? t("folders.watching") : t("folders.notWatching"),
    ];
    if (s.file_count != null) bits.push(t("folders.files", { n: s.file_count, count: formatNumber(s.file_count, lang) }));
    if (s.include_label) bits.push(s.include_label);
    else if (s.last_scan_at) bits.push(t("folders.lastScan", { when: relativeTime(s.last_scan_at, lang) }));
    return bits.join(" · ");
  };

  return (
    <div className="page-scroll">
      <div className="page">
        <div className="page-head">
          <div>
            <h1 className="page-title">{t("nav.folders")}</h1>
            <p className="page-sub">{t("folders.sub")}</p>
          </div>
          <button type="button" className="btn btn-primary" onClick={() => setAdding(true)}><Icon name="plus" size={16} stroke={1.8} />{t("folders.add")}</button>
        </div>

        <div className="stats">
          <div className="panel stat"><div className="k">{t("folders.statFolders")}</div><div className="v">{stats.data?.scope_count ?? "—"}</div></div>
          <div className="panel stat"><div className="k">{t("folders.statFiles")}</div><div className="v">{stats.data?.file_count != null ? formatNumber(stats.data.file_count, lang) : "—"}</div></div>
          <div className="panel stat"><div className="k">{t("folders.statSize")}</div><div className="v">{stats.data?.index_mb != null ? formatMb(stats.data.index_mb, lang) : "—"}</div></div>
          <div className="panel stat" style={{ gap: 8 }}>
            <div className="k">{t("folders.indexing")}</div>
            <Segmented
              label={t("folders.indexing")}
              value={settings.indexing_mode}
              onChange={(v) => update({ indexing_mode: v })}
              options={[{ value: "manual", label: t("folders.manual") }, { value: "when_idle", label: t("folders.idle") }]}
            />
          </div>
        </div>

        {scopes.error ? (
          <div className="card"><ErrorState error={scopes.error} onRetry={scopes.reload} /></div>
        ) : scopes.loading ? (
          <div className="panel folders"><div className="folder-row"><Skeleton h={40} /></div><div className="folder-row"><Skeleton h={40} /></div></div>
        ) : !list.length ? (
          <div className="card">
            <EmptyState icon={<FolderIcon size={40} />} title={t("folders.empty")}>
              <span>{t("folders.emptyBody")}</span>
              <button type="button" className="btn btn-primary" style={{ marginTop: 8 }} onClick={() => setAdding(true)}>{t("folders.add")}</button>
            </EmptyState>
          </div>
        ) : (
          <section className="panel folders rows" aria-label={t("folders.listLabel")}>
            {list.map((s) => {
              const open = expanded === s.scope_id;
              const tone = STATUS_TONE[s.status];
              return (
                <div className="folder-row" key={s.scope_id}>
                  <div className="folder-main">
                    <FolderIcon size={30} tone={s.status === "unreachable" ? "err" : s.watching ? "normal" : "off"} />
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="path">{s.display_path}{s.is_drive_root && <span className="badge" style={{ marginLeft: 8 }}>{t("folders.driveRoot")}</span>}</div>
                      <div className="meta">{metaLine(s)}</div>
                    </div>
                    <div className="folder-state">
                      <Status tone={tone}>
                        {s.status === "scanning" ? t("folders.scanning", { pct: Math.round((s.progress ?? 0) * 100) }) : t(`folders.status.${s.status}`)}
                      </Status>
                      {s.status === "scanning" && (
                        <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((s.progress ?? 0) * 100)} aria-label={t("folders.progressLabel", { path: s.display_path })}>
                          <span style={{ width: `${(s.progress ?? 0) * 100}%` }} />
                        </div>
                      )}
                    </div>
                    {s.status === "unreachable" ? (
                      <button type="button" className="btn btn-sm" onClick={() => act(api.relinkScope(s.scope_id))}>{t("folders.relink")}</button>
                    ) : (
                      <Toggle checked={s.watching} disabled label={t("folders.watchLabel", { path: s.display_path })} onChange={(v) => act(api.setWatching(s.scope_id, v))} />
                    )}
                    <button
                      type="button"
                      className="btn btn-icon"
                      style={open ? { borderColor: "var(--primary)", borderWidth: 1.5 } : undefined}
                      aria-label={t("folders.actionsLabel", { path: s.display_path })}
                      aria-expanded={open}
                      disabled={s.status === "revoking"}
                      onClick={() => setExpanded(open ? null : s.scope_id)}
                    >
                      <Icon name="more" size={16} />
                    </button>
                  </div>
                  {open && (
                    <div className="folder-actions">
                      {s.status === "scanning" ? (
                        <button type="button" className="btn btn-sm" onClick={() => act(api.cancelIndex(s.scope_id))}>{t("folders.stopScan")}</button>
                      ) : (
                        <button type="button" className="btn btn-sm" disabled={s.status === "unreachable"} onClick={() => act(api.startIndex(s.scope_id))}>{t("folders.scanNow")}</button>
                      )}
                      <button type="button" className="btn btn-sm" onClick={() => toast(t("folders.exclusionsLater"))}>{t("folders.exclusions", { n: s.exclusion_count })}</button>
                      <button type="button" className="btn btn-sm btn-danger" onClick={() => setRevoking(s)}>{t("folders.removePurge")}</button>
                    </div>
                  )}
                </div>
              );
            })}
          </section>
        )}
        <p className="page-foot">{t("folders.foot")}</p>
      </div>
      {adding && <AddFolderDialog onClose={() => setAdding(false)} />}
      {revoking && (
        <RevokeDialog
          scope={revoking}
          parentPath={pathOf(revoking.parent_scope_id) ?? list.find((p) => p.scope_id !== revoking.scope_id && revoking.display_path.startsWith(p.display_path + "/"))?.display_path ?? null}
          onClose={() => { setRevoking(null); setExpanded(null); }}
        />
      )}
    </div>
  );
}
