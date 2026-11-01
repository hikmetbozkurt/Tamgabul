import { createContext, useCallback, useContext, type ReactNode } from "react";
import en from "./en.json";
import tr from "./tr.json";
import type { Language } from "../bindings/Language";

export type Params = Record<string, string | number>;
const dictionaries: Record<Language, Record<string, string>> = { en, tr };

/**
 * English is the default UI language (docs/24 D10); Turkish is the second MVP
 * language. Missing keys fall back to English, then to the key itself.
 * Plurals: when `params.n` is given and `<key>.one` exists, n === 1 uses it.
 */
export function translate(lang: Language, key: string, params?: Params): string {
  const pick = (k: string) => dictionaries[lang][k] ?? dictionaries.en[k];
  let s: string | undefined;
  if (params && typeof params.n === "number") {
    s = params.n === 1 ? pick(`${key}.one`) : pick(`${key}.other`);
  }
  s = s ?? pick(key) ?? key;
  if (params) {
    s = s.replace(/\{(\w+)\}/g, (m, p) => (p in params ? String(params[p]) : m));
  }
  return s;
}

export type T = (key: string, params?: Params) => string;

const Ctx = createContext<{ lang: Language; t: T }>({
  lang: "en",
  t: (k, p) => translate("en", k, p),
});

export function I18nProvider({ lang, children }: { lang: Language; children: ReactNode }) {
  const t = useCallback<T>((k, p) => translate(lang, k, p), [lang]);
  return <Ctx.Provider value={{ lang, t }}>{children}</Ctx.Provider>;
}

export const useI18n = () => useContext(Ctx);
export const useT = () => useContext(Ctx).t;

export const dictionaryKeys = { en: Object.keys(en), tr: Object.keys(tr) };
