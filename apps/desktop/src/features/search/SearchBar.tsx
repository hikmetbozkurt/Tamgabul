import { useState, type FormEvent } from "react";
import { Icon } from "../../ui/icons";
import { useT } from "../../i18n";

type Props = {
  variant: "hero" | "follow" | "widget";
  disabled?: boolean;
  disabledText?: string;
  micAvailable: boolean;
  onSubmit: (text: string) => void;
  onMic: () => void;
  autoFocus?: boolean;
  initial?: string;
};

export function SearchBar({ variant, disabled, disabledText, micAvailable, onSubmit, onMic, autoFocus, initial }: Props) {
  const t = useT();
  const [text, setText] = useState(initial ?? "");
  const send = (e: FormEvent) => {
    e.preventDefault();
    if (!text.trim() || disabled) return;
    onSubmit(text);
    setText("");
  };
  const placeholder = disabled
    ? disabledText ?? t("search.disabled")
    : variant === "follow" ? t("search.followPlaceholder") : t("search.placeholder");
  const micLabel = micAvailable ? t("voice.hold") : t("voice.unavailable");

  return (
    <form className={`searchbar sb-${variant}${disabled ? " disabled" : ""}`} onSubmit={send} role="search">
      {variant === "follow" ? (
        <span className="spark-tile" aria-hidden="true"><Icon name="spark" size={18} color="var(--accent)" /></span>
      ) : (
        <Icon name="search" size={variant === "widget" ? 18 : 22} stroke={1.8} color={disabled ? "var(--muted-2)" : "var(--ink)"} />
      )}
      <label htmlFor={`q-${variant}`} className="sr-only">{t("search.label")}</label>
      <input
        id={`q-${variant}`}
        data-search-input
        value={text}
        disabled={disabled}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value)}
      />
      <button
        type="button"
        className={`btn-round${variant === "follow" ? " sm" : variant === "widget" ? " xs" : ""}`}
        aria-label={micLabel}
        title={micLabel}
        disabled={disabled || !micAvailable}
        onClick={onMic}
      >
        <Icon name="mic" size={variant === "hero" ? 20 : 18} stroke={1.8} />
      </button>
      {variant === "hero" && (
        <button type="submit" className="btn-round" aria-label={t("search.send")} disabled={disabled || !text.trim()} style={{ color: text.trim() ? "var(--ink)" : "var(--muted-2)" }}>
          <Icon name="arrowRight" size={20} stroke={1.8} />
        </button>
      )}
    </form>
  );
}
