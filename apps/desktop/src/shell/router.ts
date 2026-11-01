import { useEffect, useState } from "react";

export const PAGES = ["search", "memory", "models", "folders", "settings"] as const;
export type Page = (typeof PAGES)[number] | "welcome";

function parse(): Page {
  const h = window.location.hash.replace(/^#\/?/, "").split("?")[0];
  return (PAGES as readonly string[]).includes(h) || h === "welcome" ? (h as Page) : "search";
}

export function navigate(page: Page) {
  if (parse() !== page) window.location.hash = `/${page}`;
}

/** `#/models?add` → hashFlag("add") === true. */
export function hashFlag(name: string): boolean {
  const q = window.location.hash.split("?")[1] ?? "";
  return new URLSearchParams(q).has(name);
}

export function clearHashFlags() {
  const base = window.location.hash.split("?")[0];
  if (base !== window.location.hash) history.replaceState(null, "", base || "#/search");
}

export function useRoute(): Page {
  const [page, setPage] = useState<Page>(parse);
  useEffect(() => {
    const onHash = () => setPage(parse());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  return page;
}
