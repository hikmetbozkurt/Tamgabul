import { useCallback, useEffect, useState } from "react";
import type { IpcError } from "../../bindings/IpcError";
import type { RevokeMode } from "../../bindings/RevokeMode";
import type { ScopeExclusion } from "../../bindings/ScopeExclusion";
import type { ScopePreview } from "../../bindings/ScopePreview";
import type { ScopeSummary } from "../../bindings/ScopeSummary";
import { Icon } from "../../ui/icons";
import { Dialog, ErrorState, Skeleton } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { formatMb, formatNumber } from "../../lib/format";
import { pathPlaceholder } from "../../lib/platform";
import { isTauri, pickPath, toIpcError } from "../../ipc/bridge";

const BUILTIN_FOLDER_EXCLUDES = [".git", "node_modules", ".venv", "build", "target"];

/** Add folder: choose → dry-run preview → explicit grant (docs/07, J07). */
export function AddFolderDialog({ onClose }: { onClose: () => void }) {
  const { t, lang } = useI18n();
  const { api } = useData();
  const { snapshot } = useSettings();
  const toast = useToast();
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<ScopePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);

  const runPreview = async (p: string) => {
    setBusy(true);
    try {
      setPreview(await api.previewScope(p));
      setAccepted(false);
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
    }
    setBusy(false);
  };

  const browse = async () => {
    const p = await pickPath(true, t("folders.pickTitle"));
    if (p) { setPath(p); runPreview(p); }
  };

  const grant = async () => {
    if (!preview) return;
    setBusy(true);
    try {
      await api.grantScope(preview.preview_token);
      toast(t("folders.grantedToast", { path: preview.display_path }));
      onClose();
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
      setBusy(false);
    }
  };

  if (!preview) {
    return (
      <Dialog title={t("folders.addTitle")} subtitle={t("folders.addSub")} onClose={onClose}>
        <div className="field">
          <label htmlFor="scope-path">{t("folders.path")}</label>
          <div style={{ display: "flex", gap: 10 }}>
            <input id="scope-path" className="input" style={{ flex: 1 }} value={path} placeholder={pathPlaceholder("folder", snapshot.os)} onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && path.trim() && runPreview(path)} />
            {isTauri() && <button type="button" className="btn" onClick={browse}>{t("addModel.browse")}</button>}
          </div>
        </div>
        <div className="note"><Icon name="info" size={18} /><span>{t("folders.previewNote")}</span></div>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
          <button type="button" className="btn btn-primary" disabled={!path.trim() || busy} onClick={() => runPreview(path)}>{t("folders.preview")}</button>
        </div>
      </Dialog>
    );
  }

  const root = preview.is_drive_root;
  const network = preview.is_network;
  const needsRisk = root || network;
  return (
    <Dialog
      title={
        root
          ? t("folders.rootTitle", { path: preview.display_path })
          : network
            ? t("folders.networkTitle", { path: preview.display_path })
            : t("folders.confirmTitle", { path: preview.display_path })
      }
      subtitle={t("folders.previewDone")}
      icon={needsRisk ? <span className="icon-tile lg" style={{ background: "var(--warn-soft)", color: "var(--warn-text)", width: 44, height: 44 }}><Icon name="warn" size={22} stroke={1.8} /></span> : undefined}
      onClose={onClose}
    >
      <div className="preview-stats">
        <div><div className="k">{t("folders.found")}</div><div className="v">{t("folders.files", { n: preview.file_count, count: formatNumber(preview.file_count, lang) + (preview.complete ? "" : "+") })}</div></div>
        <div><div className="k">{t("folders.estIndex")}</div><div className="v">{formatMb(preview.est_index_mb, lang)}</div></div>
        <div><div className="k">{t("folders.cap")}</div><div className="v">{formatNumber(preview.cap_files, lang)}</div></div>
      </div>
      <div className="field">
        <span className="field-label">{t("folders.autoExcluded")}</span>
        <div className="excludes">{preview.auto_excludes.map((x) => <span key={x}>{x}</span>)}</div>
      </div>
      {preview.parent_display_path && (
        <div className="note"><Icon name="info" size={18} /><span>{t("folders.insideParent", { parent: preview.parent_display_path })}</span></div>
      )}
      {network && (
        <div className="note"><Icon name="info" size={18} /><span>{t("folders.networkNote")}</span></div>
      )}
      {needsRisk && (
        <label className="check risk">
          <input type="checkbox" checked={accepted} onChange={(e) => setAccepted(e.target.checked)} />
          <span>{t(root ? "folders.riskAccept" : "folders.networkRiskAccept", { path: preview.display_path })}</span>
        </label>
      )}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={() => setPreview(null)}>{t("common.back")}</button>
        <button type="button" className="btn btn-primary" disabled={busy || (needsRisk && !accepted)} onClick={grant}>{t("folders.grant")}</button>
      </div>
    </Dialog>
  );
}

/** Remove: distinguishes stop-watching from erase; child-under-parent choice (J08). */
export function RevokeDialog({ scope, parentPath, onClose }: { scope: ScopeSummary; parentPath: string | null; onClose: () => void }) {
  const { t } = useI18n();
  const { api } = useData();
  const toast = useToast();
  const child = !!parentPath;
  const [mode, setMode] = useState<RevokeMode>(child ? "block_and_purge" : "remove_and_purge");
  const [purgeHistory, setPurgeHistory] = useState(false);
  const [busy, setBusy] = useState(false);

  const options: { v: RevokeMode; title: string; body: string }[] = child
    ? [
        { v: "block_and_purge", title: t("revoke.block"), body: t("revoke.blockBody") },
        { v: "remove_grant_only", title: t("revoke.grantOnly"), body: t("revoke.grantOnlyBody", { parent: parentPath! }) },
      ]
    : [
        { v: "remove_and_purge", title: t("revoke.purge"), body: t("revoke.purgeBody") },
        { v: "stop_watching", title: t("revoke.stop"), body: t("revoke.stopBody") },
      ];

  const go = async () => {
    setBusy(true);
    try {
      await api.revokeScope({ scope_id: scope.scope_id, mode, purge_history: purgeHistory });
      toast(t(mode === "stop_watching" ? "revoke.stoppedToast" : "revoke.removedToast", { path: scope.display_path }));
      onClose();
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
      setBusy(false);
    }
  };

  const destructive = mode !== "stop_watching";
  return (
    <Dialog title={t("revoke.title", { path: scope.display_path })} onClose={onClose} small>
      {child && <p style={{ fontSize: 14.5, lineHeight: 1.5, color: "var(--ink-2)" }}>{t("revoke.insideParent", { parent: parentPath! })}</p>}
      <div role="radiogroup" aria-label={t("revoke.how")} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {options.map((o) => (
          <label key={o.v} className="radio-card">
            <input type="radio" name="revoke" checked={mode === o.v} onChange={() => setMode(o.v)} />
            <span><strong>{o.title}</strong><br /><span className="muted">{o.body}</span></span>
          </label>
        ))}
      </div>
      {destructive && (
        <label className="check" style={{ fontSize: 14, color: "var(--ink-2)" }}>
          <input type="checkbox" checked={purgeHistory} onChange={(e) => setPurgeHistory(e.target.checked)} />
          {t("revoke.history")}
        </label>
      )}
      <p className="page-foot">{t("revoke.note")}</p>
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
        <button type="button" className={`btn ${destructive ? "btn-danger-solid" : "btn-primary"}`} disabled={busy} onClick={go}>
          {destructive ? t("revoke.confirm") : t("revoke.stop")}
        </button>
      </div>
    </Dialog>
  );
}

/** Per-folder custom exclusions + read-only built-in exclusions (docs/07). */
export function ExclusionsDialog({ scope, onClose }: { scope: ScopeSummary; onClose: () => void }) {
  const { t } = useI18n();
  const { api } = useData();
  const toast = useToast();
  const [items, setItems] = useState<ScopeExclusion[] | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      setItems(await api.listScopeExclusions(scope.scope_id));
    } catch (e) {
      setError(toIpcError(e));
    }
  }, [api, scope.scope_id]);

  useEffect(() => {
    load();
  }, [load]);

  const add = async (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      const created = await api.addScopeExclusion(scope.scope_id, trimmed);
      setPath("");
      toast(t("exclusions.addedToast", { path: created.display_path }));
      await load();
    } catch (e) {
      const err = toIpcError(e);
      toast(err.code === "SCOPE_NOT_AUTHORIZED" ? t("exclusions.invalidHint") : t(`error.${err.code}.title`), true);
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    const p = await pickPath(true, t("exclusions.pickTitle"), scope.display_path);
    if (p) {
      setPath(p);
      add(p);
    }
  };

  const remove = async (ex: ScopeExclusion) => {
    setBusy(true);
    try {
      await api.removeScopeExclusion(scope.scope_id, ex.exclusion_id);
      toast(t("exclusions.removedToast", { path: ex.display_path }));
      await load();
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
    } finally {
      setBusy(false);
    }
  };

  const builtin = [
    ...BUILTIN_FOLDER_EXCLUDES,
    t("exclusions.builtinHidden"),
    t("exclusions.builtinLinks"),
    t("exclusions.builtinCloud"),
    ...(scope.is_drive_root ? [t("exclusions.builtinOs")] : []),
  ];

  return (
    <Dialog title={t("exclusions.title")} subtitle={scope.display_path} onClose={onClose}>
      <div className="field">
        <label htmlFor="excl-path">{t("exclusions.pathLabel")}</label>
        <div style={{ display: "flex", gap: 10 }}>
          <input
            id="excl-path"
            className="input"
            style={{ flex: 1 }}
            value={path}
            placeholder={t("exclusions.pathPlaceholder")}
            disabled={busy}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && path.trim() && add(path)}
          />
          {isTauri() && <button type="button" className="btn" disabled={busy} onClick={browse}>{t("addModel.browse")}</button>}
          <button type="button" className="btn btn-primary" disabled={!path.trim() || busy} onClick={() => add(path)}>{t("exclusions.add")}</button>
        </div>
      </div>

      <div className="field">
        <span className="field-label">{t("exclusions.customLabel")}</span>
        {error ? (
          <ErrorState error={error} onRetry={load} />
        ) : !items ? (
          <Skeleton h={48} />
        ) : !items.length ? (
          <p className="small muted">{t("exclusions.empty")}</p>
        ) : (
          <ul className="exclusion-list rows">
            {items.map((ex) => (
              <li className="exclusion-row" key={ex.exclusion_id}>
                <span className="exclusion-path">{ex.display_path}</span>
                <span className="badge">{t(`exclusions.reason.${ex.reason}`)}</span>
                {ex.reason !== "protected_path" && (
                  <button
                    type="button"
                    className="btn btn-xs btn-danger"
                    disabled={busy}
                    aria-label={t("exclusions.removeLabel", { path: ex.display_path })}
                    onClick={() => remove(ex)}
                  >
                    {t("exclusions.remove")}
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="field">
        <span className="field-label">{t("exclusions.builtinLabel")}</span>
        <div className="excludes">{builtin.map((x) => <span key={x}>{x}</span>)}</div>
      </div>

      <p className="page-foot">{t("exclusions.foot")}</p>
      <div className="dialog-actions">
        <button type="button" className="btn btn-primary" onClick={onClose}>{t("common.close")}</button>
      </div>
    </Dialog>
  );
}
