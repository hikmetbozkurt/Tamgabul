import { useEffect, useRef, type ReactNode } from "react";
import type { StatusTone } from "../bindings/StatusTone";
import type { IpcError } from "../bindings/IpcError";
import { useT } from "../i18n";
import { Icon } from "./icons";

export const TONE_CLASS: Record<StatusTone, string> = {
  green: "ok",
  amber: "warn",
  red: "err",
  neutral: "neutral",
};

/** A colored dot never appears without text (docs/09). */
export function Status({ tone, children }: { tone: StatusTone; children: ReactNode }) {
  return (
    <span className={`status${tone === "red" ? " err" : ""}`}>
      <span className={`dot dot-${TONE_CLASS[tone]}`} />
      {children}
    </span>
  );
}

export function Pill({ tone, children, large }: { tone: StatusTone; children: ReactNode; large?: boolean }) {
  return (
    <span className={`pill pill-${TONE_CLASS[tone]}${large ? " lg" : ""}`}>
      <span className={`dot dot-${TONE_CLASS[tone]}`} />
      {children}
    </span>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="toggle"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

export function Segmented<V extends string>({
  value,
  options,
  onChange,
  label,
  large,
}: {
  value: V;
  options: { value: V; label: ReactNode }[];
  onChange: (v: V) => void;
  label: string;
  large?: boolean;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={`seg${large ? " lg" : ""}`}
      onKeyDown={(e) => {
        // Radio group pattern: one Tab stop, arrows move and select.
        const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
        if (!step) return;
        e.preventDefault();
        const i = options.findIndex((o) => o.value === value);
        const next = options[(i + step + options.length) % options.length];
        onChange(next.value);
        const btns = e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=radio]");
        btns[options.indexOf(next)]?.focus();
      }}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={o.value === value || (!options.some((x) => x.value === value) && o === options[0]) ? 0 : -1}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Modal dialog: focus moves in, Tab stays inside, Esc and scrim click close. */
export function Dialog({
  title,
  subtitle,
  icon,
  onClose,
  children,
  small,
}: {
  title: string;
  subtitle?: string;
  icon?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  small?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const t = useT();
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const el = ref.current;
    const focusables = () =>
      Array.from(
        el?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select, [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    (focusables()[1] ?? focusables()[0])?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
      if (e.key === "Tab") {
        const f = focusables();
        if (!f.length) return;
        const first = f[0], last = f[f.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
  }, [onClose]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} role="dialog" aria-modal="true" aria-labelledby="dlg-title" className={`dialog${small ? " sm" : ""}`}>
        <div className="dialog-head">
          <div style={{ display: "flex", gap: 14, alignItems: "center" }}>
            {icon}
            <div>
              <h2 id="dlg-title" className="dialog-title">{title}</h2>
              {subtitle && <div className="dialog-sub">{subtitle}</div>}
            </div>
          </div>
          <button type="button" className="btn btn-icon" aria-label={t("common.close")} onClick={onClose}>
            <Icon name="close" size={16} stroke={1.8} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function EmptyState({ icon, title, children }: { icon?: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      {icon}
      <strong>{title}</strong>
      {children}
    </div>
  );
}

/** Honest error: localized reason from the code + one action. */
export function ErrorState({ error, onRetry }: { error: IpcError; onRetry?: () => void }) {
  const t = useT();
  return (
    <div className="empty" role="alert">
      <span className="icon-tile md err"><Icon name="warn" size={20} /></span>
      <strong>{t(`error.${error.code}.title`)}</strong>
      <span>{t(`error.${error.code}.body`)}</span>
      {onRetry && error.code !== "NOT_IMPLEMENTED" && (
        <button type="button" className="btn btn-sm" onClick={onRetry}>{t("common.retry")}</button>
      )}
    </div>
  );
}

export function Skeleton({ h = 20, w = "100%", r }: { h?: number; w?: number | string; r?: number }) {
  return <div className="skeleton" style={{ height: h, width: w, borderRadius: r }} aria-hidden="true" />;
}
