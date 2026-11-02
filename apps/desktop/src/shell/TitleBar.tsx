import { useEffect, useRef, useState } from "react";
import { Icon, Logo } from "../ui/icons";
import { Status } from "../ui/primitives";
import { useT } from "../i18n";
import { useResource } from "../state/data";
import { hideSelf, openWidget, toggleMaximize } from "../ipc/bridge";
import { statusKey } from "../lib/labels";
import type { StatusTone } from "../bindings/StatusTone";

const LEVEL_TONE: Record<string, StatusTone> = { ready: "green", degraded: "amber", blocked: "red" };

/** Custom title bar (docs/24 D5): drag region, health menu, window buttons. */
export function TitleBar({ onboarding }: { onboarding?: boolean }) {
  const t = useT();
  const health = useResource((a) => a.health());
  const [open, setOpen] = useState(false);
  const popRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === "Escape" : !popRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);

  const h = health.data;
  const tone: StatusTone = h ? LEVEL_TONE[h.level] : "neutral";
  const label = !h ? t("health.loading") : onboarding || h.level === "blocked" ? t("health.setupNeeded") : t("health.local");

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span data-tauri-drag-region>Tamgabul</span>
      </div>
      <div className="titlebar-right">
        <button
          type="button"
          className="status-trigger"
          aria-haspopup="dialog"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          <Status tone={tone}><span style={{ color: tone === "red" ? undefined : "var(--ink)", fontSize: 14.5 }}>{label}</span></Status>
          <Icon name="chevronDown" size={14} stroke={2} />
        </button>
        {open && h && (
          <div ref={popRef} className="popover" role="dialog" aria-label={t("health.menuTitle")}>
            <h3>{t(`health.level.${h.level}`)}</h3>
            {h.models.map((m) => (
              <div className="row" key={m.category}>
                <span>{t(`category.${m.category}`)}</span>
                <Status tone={m.tone}>{t(statusKey(m.status, m.required))}</Status>
              </div>
            ))}
            <div className="row">
              <span>{t("health.folders")}</span>
              <span className="small muted">{t("health.folderCount", { n: h.active_scope_count })}</span>
            </div>
            <div className="note"><Icon name="globe" size={16} />{t("health.offline")}</div>
          </div>
        )}
        <span className="titlebar-sep" />
        <button type="button" className="winbtn" aria-label={t("window.toWidget")} title={t("window.toWidget")} onClick={() => openWidget()}>
          <Icon name="minimize" size={16} stroke={1.6} />
        </button>
        <button type="button" className="winbtn" aria-label={t("window.maximize")} onClick={() => toggleMaximize()}>
          <Icon name="maximize" size={15} stroke={1.6} />
        </button>
        <button type="button" className="winbtn close" aria-label={t("window.hide")} title={t("window.hide")} onClick={() => hideSelf()}>
          <Icon name="close" size={17} stroke={1.6} />
        </button>
      </div>
    </header>
  );
}
