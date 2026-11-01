import { useEffect, useRef } from "react";
import { Icon } from "../../ui/icons";
import { ErrorState, Skeleton } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useResource } from "../../state/data";
import { relativeTime } from "../../lib/format";
import { FILE_ICON, FILE_KIND_KEY } from "../../lib/labels";
import { Snippet } from "./ResultCard";

/** Right-side preview panel (user decision: panel, not a separate window). */
export function PreviewDrawer({
  resultRef,
  onClose,
  onOpen,
  onReveal,
}: {
  resultRef: string;
  onClose: () => void;
  onOpen: (ref: string) => void;
  onReveal: (ref: string) => void;
}) {
  const { t, lang } = useI18n();
  const res = useResource((a) => a.preview(resultRef), [resultRef]);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const r = res.data?.result;
  return (
    <div ref={ref} className="drawer" role="complementary" aria-label={t("preview.label")} tabIndex={-1}>
      <div className="drawer-head">
        {r ? (
          <>
            <span className="icon-tile file" style={{ width: 52, height: 52 }}><Icon name={FILE_ICON[r.file_kind]} size={24} /></span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="result-title" style={{ fontSize: 18 }}>{r.title}</div>
              <div className="result-path" style={{ marginTop: 4 }}>{r.display_path}</div>
            </div>
          </>
        ) : (
          <div style={{ flex: 1 }}><Skeleton h={22} w="60%" /></div>
        )}
        <button type="button" className="btn btn-icon" aria-label={t("common.close")} onClick={onClose}>
          <Icon name="close" size={16} stroke={1.8} />
        </button>
      </div>
      <div className="drawer-body">
        {res.error ? (
          <ErrorState error={res.error} />
        ) : !res.data || !r ? (
          <>
            <Skeleton h={60} />
            <Skeleton h={180} />
          </>
        ) : (
          <>
            <div className="meta-grid">
              <div><div className="k">{t("preview.project")}</div><div>{r.project ?? "—"}</div></div>
              <div><div className="k">{t("preview.type")}</div><div>{t(FILE_KIND_KEY[r.file_kind])}</div></div>
              <div><div className="k">{t("preview.modified")}</div><div>{relativeTime(r.modified_at, lang)}</div></div>
              <div><div className="k">{t("preview.why")}</div><div>{t(`match.${r.match_reason}`)}</div></div>
            </div>
            {r.snippet && (
              <div className="note"><Icon name="search" size={16} /><span><Snippet text={r.snippet} marks={r.highlights} /></span></div>
            )}
            <div className="drawer-text">{res.data.body}</div>
            <p className="page-foot">{t("preview.readOnly")}</p>
          </>
        )}
      </div>
      <div className="drawer-foot">
        <button type="button" className="btn" onClick={() => onReveal(resultRef)}><Icon name="reveal" size={17} />{t("result.reveal")}</button>
        <button type="button" className="btn btn-primary" onClick={() => onOpen(resultRef)}><Icon name="external" size={17} stroke={1.8} />{t("result.open")}</button>
      </div>
    </div>
  );
}
