import type { ResultCard as Card } from "../../bindings/ResultCard";
import { Icon } from "../../ui/icons";
import { useI18n } from "../../i18n";
import { relativeTime } from "../../lib/format";
import { FILE_ICON, FILE_KIND_KEY } from "../../lib/labels";

/** Snippet with highlighted substrings, rendered as text (never HTML). */
export function Snippet({ text, marks }: { text: string; marks: string[] }) {
  if (!marks.length) return <>{text}</>;
  const esc = marks.map((m) => m.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const parts = text.split(new RegExp(`(${esc.join("|")})`, "gi"));
  return (
    <>
      {parts.map((p, i) =>
        marks.some((m) => m.toLowerCase() === p.toLowerCase()) ? <mark key={i}>{p}</mark> : <span key={i}>{p}</span>,
      )}
    </>
  );
}

type Actions = {
  onOpen: (ref: string) => void;
  onReveal: (ref: string) => void;
  onPreview: (ref: string) => void;
};

export function ResultCard({ r, opened, ...a }: { r: Card; opened?: boolean } & Actions) {
  const { t, lang } = useI18n();
  return (
    <article
      className="card result"
      tabIndex={0}
      data-result
      aria-label={r.title}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) a.onOpen(r.result_ref);
        if (e.key === " " && e.target === e.currentTarget) { e.preventDefault(); a.onPreview(r.result_ref); }
      }}
    >
      <div className="result-top">
        <span className="icon-tile file tile"><Icon name={FILE_ICON[r.file_kind]} size={30} stroke={1.6} /></span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="result-title">{r.title}</div>
          <div className="result-path">{r.display_path}</div>
          {r.snippet && (
            <p className="result-snippet"><Snippet text={r.snippet} marks={r.highlights} /></p>
          )}
        </div>
        <button type="button" className="btn btn-icon" aria-label={t("result.reveal")} title={t("result.reveal")} onClick={() => a.onReveal(r.result_ref)}>
          <Icon name="external" size={17} stroke={1.8} />
        </button>
      </div>
      <div className="result-foot">
        {r.project && <span className="chip chip-meta"><Icon name="folder" size={17} />{r.project}</span>}
        <span className="chip chip-meta"><Icon name="fileText" size={16} />{t(FILE_KIND_KEY[r.file_kind])}</span>
        <span className="chip chip-meta"><Icon name="clock" size={17} />{relativeTime(r.modified_at, lang)}</span>
        {opened && <span className="opened-badge"><Icon name="check" size={14} stroke={2.4} />{t("result.opened")}</span>}
        <div className="result-actions">
          <button type="button" className="btn" style={{ padding: "0 20px" }} onClick={() => a.onPreview(r.result_ref)}>
            <Icon name="eye" size={18} />{t("result.preview")}
          </button>
          <button type="button" className="btn btn-primary" style={{ padding: "0 26px" }} onClick={() => a.onOpen(r.result_ref)}>
            <Icon name="external" size={17} stroke={1.8} />{t("result.open")}
          </button>
        </div>
      </div>
    </article>
  );
}

/** Widget row: icon, title, project · path · age, one action. */
export function CompactResult({ r, onOpen, primary }: { r: Card; onOpen: (ref: string) => void; primary?: boolean }) {
  const { t, lang } = useI18n();
  const meta = [r.project, r.display_path.split("/").slice(-2).join("/"), relativeTime(r.modified_at, lang)].filter(Boolean).join(" · ");
  return (
    <article className="card w-result" data-result tabIndex={0} onKeyDown={(e) => e.key === "Enter" && e.target === e.currentTarget && onOpen(r.result_ref)}>
      <span className={`icon-tile ${r.file_kind === "image" ? "" : "file"} w-tile`}>
        <Icon name={FILE_ICON[r.file_kind]} size={21} stroke={1.6} />
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="w-title">{r.title}</div>
        <div className="w-meta">{meta}</div>
      </div>
      <button type="button" className={`btn btn-xs${primary ? " btn-primary" : ""}`} onClick={() => onOpen(r.result_ref)}>
        {t("result.open")}
      </button>
    </article>
  );
}
