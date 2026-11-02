import { useState } from "react";
import type { RevokeMode } from "../../bindings/RevokeMode";
import type { ScopePreview } from "../../bindings/ScopePreview";
import type { ScopeSummary } from "../../bindings/ScopeSummary";
import { Icon } from "../../ui/icons";
import { Dialog } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData } from "../../state/data";
import { useToast } from "../../state/toast";
import { formatMb, formatNumber } from "../../lib/format";
import { isTauri, pickPath, toIpcError } from "../../ipc/bridge";

/** Add folder: choose → dry-run preview → explicit grant (docs/07, J07). */
export function AddFolderDialog({ onClose }: { onClose: () => void }) {
  const { t, lang } = useI18n();
  const { api } = useData();
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
            <input id="scope-path" className="input" style={{ flex: 1 }} value={path} placeholder="C:/Projects" onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && path.trim() && runPreview(path)} />
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
  return (
    <Dialog
      title={root ? t("folders.rootTitle", { path: preview.display_path }) : t("folders.confirmTitle", { path: preview.display_path })}
      subtitle={t("folders.previewDone")}
      icon={root ? <span className="icon-tile lg" style={{ background: "var(--warn-soft)", color: "var(--warn-text)", width: 44, height: 44 }}><Icon name="warn" size={22} stroke={1.8} /></span> : undefined}
      onClose={onClose}
    >
      <div className="preview-stats">
        <div><div className="k">{t("folders.found")}</div><div className="v">{t("folders.files", { n: preview.file_count, count: formatNumber(preview.file_count, lang) })}</div></div>
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
      {root && (
        <label className="check risk">
          <input type="checkbox" checked={accepted} onChange={(e) => setAccepted(e.target.checked)} />
          {t("folders.riskAccept", { path: preview.display_path })}
        </label>
      )}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={() => setPreview(null)}>{t("common.back")}</button>
        <button type="button" className="btn btn-primary" disabled={busy || (root && !accepted)} onClick={grant}>{t("folders.grant")}</button>
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
