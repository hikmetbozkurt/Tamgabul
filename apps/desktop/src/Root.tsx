import { useEffect, type ReactNode } from "react";
import type { Api } from "./ipc/api";
import type { AppSettings } from "./bindings/AppSettings";
import type { SettingsSnapshot } from "./bindings/SettingsSnapshot";
import { I18nProvider } from "./i18n";
import { SettingsProvider, useSettings } from "./state/settings";
import { DataProvider } from "./state/data";
import { ToastProvider } from "./state/toast";
import type { Surface } from "./ipc/bridge";
import { AppSurface } from "./surfaces/AppSurface";
import { WidgetSurface } from "./surfaces/WidgetSurface";

function WithLanguage({ children }: { children: ReactNode }) {
  const { settings } = useSettings();
  useEffect(() => {
    document.documentElement.lang = settings.language;
  }, [settings.language]);
  return <I18nProvider lang={settings.language}>{children}</I18nProvider>;
}

export function Root({
  surface,
  api,
  initialSnapshot,
  onUpdateSettings,
}: {
  surface: Surface;
  api?: Api;
  initialSnapshot?: SettingsSnapshot;
  onUpdateSettings?: (next: AppSettings) => Promise<SettingsSnapshot>;
}) {
  return (
    <SettingsProvider initialSnapshot={initialSnapshot} onUpdate={onUpdateSettings}>
      <WithLanguage>
        <DataProvider api={api}>
          <ToastProvider>
            {surface === "widget" ? <WidgetSurface /> : <AppSurface />}
          </ToastProvider>
        </DataProvider>
      </WithLanguage>
    </SettingsProvider>
  );
}
