import { useEffect } from "react";
import { TitleBar } from "../shell/TitleBar";
import { SideNav } from "../shell/SideNav";
import { PAGES, navigate, useRoute, type Page } from "../shell/router";
import { on } from "../ipc/bridge";
import { useResource } from "../state/data";
import { SearchPage } from "../features/search/SearchPage";
import { MemoryPage } from "../features/memory/MemoryPage";
import { ModelsPage } from "../features/models/ModelsPage";
import { FoldersPage } from "../features/folders/FoldersPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { OnboardingPage } from "../features/onboarding/OnboardingPage";
import { useT } from "../i18n";

const WELCOMED = "tamgabul.welcomed";

export function AppSurface() {
  const page = useRoute();
  const t = useT();
  const health = useResource((a) => a.health());

  // Rust asks us to open a page (tray "Settings", widget buttons).
  useEffect(() => {
    const p = on<string>("navigate", (route) => navigate(route as Page));
    return () => { p.then((u) => u()); };
  }, []);

  // J01: with no embedding model, open onboarding once per app run.
  useEffect(() => {
    const emb = health.data?.models.find((m) => m.category === "embedding");
    if (emb?.status === "MISSING" && !sessionStorage.getItem(WELCOMED)) {
      sessionStorage.setItem(WELCOMED, "1");
      navigate("welcome");
    }
  }, [health.data]);

  // Ctrl+K focuses search; Ctrl+1…5 switches pages (docs/24 §5).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "k") {
        e.preventDefault();
        navigate("search");
        setTimeout(() => document.querySelector<HTMLInputElement>("[data-search-input]")?.focus(), 0);
      }
      const i = Number(e.key) - 1;
      if (i >= 0 && i < PAGES.length) {
        e.preventDefault();
        navigate(PAGES[i]);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app" style={{ position: "relative" }}>
      {page !== "welcome" && (
        <a className="skip-link" href="#main" onClick={(e) => { e.preventDefault(); document.getElementById("main")?.focus(); }}>
          {t("a11y.skipToContent")}
        </a>
      )}
      <TitleBar onboarding={page === "welcome"} />
      {page === "welcome" ? (
        <OnboardingPage />
      ) : (
        <div className="app-body">
          <SideNav page={page} />
          <main id="main" className="app-main" tabIndex={-1} aria-label={t(`nav.${page}`)}>
            {page === "search" && <SearchPage />}
            {page === "memory" && <MemoryPage />}
            {page === "models" && <ModelsPage />}
            {page === "folders" && <FoldersPage />}
            {page === "settings" && <SettingsPage />}
          </main>
        </div>
      )}
    </div>
  );
}
