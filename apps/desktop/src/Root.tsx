import { useEffect, type ReactNode } from "react";
import { I18nProvider } from "./i18n";
import { SettingsProvider, useSettings } from "./state/settings";
import { DataProvider } from "./state/data";
import { ToastProvider } from "./state/toast";
import type { Surface } from "./ipc/bridge";
import { AppSurface } from "./surfaces/AppSurface";
import { WidgetSurface } from "./surfaces/WidgetSurface";
import { GallerySurface } from "./surfaces/GallerySurface";

function WithLanguage({ children }: { children: ReactNode }) {
  const { settings } = useSettings();
  useEffect(() => {
    document.documentElement.lang = settings.language;
  }, [settings.language]);
  return <I18nProvider lang={settings.language}>{children}</I18nProvider>;
}

export function Root({ surface }: { surface: Surface }) {
  return (
    <SettingsProvider>
      <WithLanguage>
        <DataProvider>
          <ToastProvider>
            {surface === "widget" ? <WidgetSurface /> : surface === "gallery" ? <GallerySurface /> : <AppSurface />}
          </ToastProvider>
        </DataProvider>
      </WithLanguage>
    </SettingsProvider>
  );
}
