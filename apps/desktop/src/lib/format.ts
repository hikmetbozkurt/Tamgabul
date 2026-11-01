import type { Language } from "../bindings/Language";

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 86_400_000],
  ["month", 30 * 86_400_000],
  ["week", 7 * 86_400_000],
  ["day", 86_400_000],
  ["hour", 3_600_000],
  ["minute", 60_000],
];

/** "2 days ago" / "2 gün önce" */
export function relativeTime(isoDate: string, lang: Language, now = Date.now()): string {
  const diff = new Date(isoDate).getTime() - now;
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  for (const [unit, ms] of UNITS) {
    if (Math.abs(diff) >= ms) return rtf.format(Math.round(diff / ms), unit);
  }
  return rtf.format(0, "minute");
}

export function clockTime(isoDate: string, lang: Language): string {
  return new Date(isoDate).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
}

export function weekday(isoDate: string, lang: Language): string {
  return new Date(isoDate).toLocaleDateString(lang, { weekday: "long" });
}

/** Day bucket for history grouping. */
export function dayBucket(isoDate: string, now = new Date()): "today" | "yesterday" | "week" | "older" {
  const d = new Date(isoDate);
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const t = d.getTime();
  if (t >= start) return "today";
  if (t >= start - 86_400_000) return "yesterday";
  if (t >= start - 7 * 86_400_000) return "week";
  return "older";
}

export function formatNumber(n: number, lang: Language): string {
  return new Intl.NumberFormat(lang).format(n);
}

export function formatMb(mb: number, lang: Language): string {
  return mb >= 1024
    ? `${new Intl.NumberFormat(lang, { maximumFractionDigits: 1 }).format(mb / 1024)} GB`
    : `${new Intl.NumberFormat(lang, { maximumFractionDigits: 1 }).format(mb)} MB`;
}
