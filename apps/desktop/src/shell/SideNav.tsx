import { Icon, type IconName } from "../ui/icons";
import { useT } from "../i18n";
import { PAGES, type Page } from "./router";

const ICONS: Record<(typeof PAGES)[number], IconName> = {
  search: "search",
  memory: "memory",
  models: "models",
  folders: "folder",
  settings: "settings",
};

export function SideNav({ page }: { page: Page }) {
  const t = useT();
  return (
    <nav className="sidenav" aria-label={t("nav.label")}>
      {PAGES.map((p, i) => (
        <a
          key={p}
          href={`#/${p}`}
          className="navitem"
          aria-current={page === p ? "page" : undefined}
          title={`${t(`nav.${p}`)} (Ctrl+${i + 1})`}
        >
          <Icon name={ICONS[p]} size={22} />
          {t(`nav.${p}`)}
        </a>
      ))}
      <div className="privacy-card">
        <Icon name="spark" size={22} color="var(--spark)" />
        <strong>{t("privacy.title")}</strong>
        <span>{t("privacy.body")}</span>
      </div>
    </nav>
  );
}
