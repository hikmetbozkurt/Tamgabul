import { useEffect, useState } from "react";
import type { AppSettings } from "../../bindings/AppSettings";
import type { PingResponse } from "../../bindings/PingResponse";
import { Flag } from "../../ui/flags";
import { Dialog, Segmented, Status, Toggle } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData, useResource } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { formatMb } from "../../lib/format";
import { isLoaded } from "../../lib/health";
import { call, isTauri, pickSavePath, toIpcError } from "../../ipc/bridge";
import { LICENSES } from "../../lib/licenses";
import type { ResourceUsage } from "../../bindings/ResourceUsage";
import { currentSession } from "../../ipc/api";

const MODIFIERS: Record<string, string> = { Control: "Ctrl", Shift: "Shift", Alt: "Alt", Meta: "Win" };

/** Keydown → "Ctrl+Shift+Space"; null until a non-modifier key arrives. */
export function comboFromEvent(e: { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }): string | null {
  if (e.key in MODIFIERS) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean) as string[];
  if (!mods.length) return null;
  const key = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [...mods, key].join("+");
}

function HotkeyField() {
  const { t } = useI18n();
  const { settings, snapshot, update } = useSettings();
  const toast = useToast();
  const [capturing, setCapturing] = useState(false);
  const [conflict, setConflict] = useState<string | null>(null);

  const onKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!capturing) return;
    e.preventDefault();
    if (e.key === "Escape") { setCapturing(false); return; }
    const combo = comboFromEvent(e);
    if (!combo) return;
    setCapturing(false);
    try {
      await update({ hotkey: combo });
      setConflict(null);
      toast(t("settings.hotkeySaved", { key: combo }));
    } catch (err) {
      const c = toIpcError(err);
      setConflict(c.code === "HOTKEY_CONFLICT" ? combo : null);
      if (c.code !== "HOTKEY_CONFLICT") toast(t(`error.${c.code}.title`), true);
    }
  };

  const warn = conflict || !snapshot.hotkey_active;
  return (
    <div className="field">
      <label htmlFor="hotkey">{t("settings.hotkey")}</label>
      <div style={{ display: "flex", gap: 10 }}>
        <input
          id="hotkey"
          className={`input${warn ? " warn" : ""}`}
          style={{ flex: 1 }}
          readOnly
          value={capturing ? t("settings.pressKeys") : settings.hotkey.replace(/\+/g, " + ")}
          onKeyDown={onKeyDown}
          onBlur={() => setCapturing(false)}
          aria-describedby="hotkey-hint"
        />
        <button
          type="button"
          className="btn btn-sm"
          onClick={() => { setCapturing(true); document.getElementById("hotkey")?.focus(); }}
        >
          {t("settings.change")}
        </button>
      </div>
      <span id="hotkey-hint" className={`field-hint${warn ? " warn" : ""}`}>
        {conflict ? t("settings.hotkeyConflict", { key: conflict }) : !snapshot.hotkey_active ? t("settings.hotkeyInactive") : t("settings.hotkeyHint")}
      </span>
    </div>
  );
}

export function SettingsPage() {
  const { t, lang } = useI18n();
  const { settings, snapshot, update } = useSettings();
  const { api } = useData();
  const toast = useToast();
  const models = useResource((a) => a.listModels());
  const stats = useResource((a) => a.indexStats());
  const health = useResource((a) => a.health());
  const [info, setInfo] = useState<PingResponse | null>(null);
  const [wipe, setWipe] = useState(false);
  const [licenses, setLicenses] = useState(false);
  const [usage, setUsage] = useState<ResourceUsage | null>(null);

  useEffect(() => {
    if (isTauri()) call<PingResponse>("ping", { request: { request_id: "settings" } }).then(setInfo).catch(() => {});
  }, []);

  // Measured resource use, refreshed while this page is open.
  useEffect(() => {
    let alive = true;
    const tick = () => api.resourceUsage().then((u) => alive && setUsage(u)).catch(() => {});
    tick();
    const id = window.setInterval(tick, 5000);
    return () => { alive = false; window.clearInterval(id); };
  }, [api]);

  const exportDiagnostics = async () => {
    try {
      const path = await pickSavePath("tamgabul-diagnostics.json", t("settings.exportDiag"));
      if (!path) return;
      await api.exportDiagnostics(path);
      toast(t("settings.diagSaved"));
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
    }
  };

  const showDataFolder = () => api.openDataFolder().catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));

  const set = (patch: Partial<AppSettings>) =>
    update(patch).catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));

  const loaded = (models.data ?? []).filter((m) => m.status !== "MISSING");

  const wipeAll = async () => {
    try {
      await api.clearHistory();
      currentSession.set(null);
      toast(t("settings.wipedToast"));
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
    }
    setWipe(false);
  };

  return (
    <div className="page-scroll">
      <div className="page">
        <div>
          <h1 className="page-title">{t("nav.settings")}</h1>
          <p className="page-sub">{t("settings.sub")}</p>
        </div>

        <div className="settings-grid">
          <section className="card settings-card" aria-labelledby="s-general">
            <h2 id="s-general">{t("settings.general")}</h2>
            <div className="field">
              <span className="field-label">{t("settings.language")}</span>
              <Segmented
                large
                label={t("settings.language")}
                value={settings.language}
                onChange={(v) => set({ language: v })}
                options={[
                  { value: "en", label: <span className="lang-option"><Flag lang="en" />English</span> },
                  { value: "tr", label: <span className="lang-option"><Flag lang="tr" />Türkçe</span> },
                ]}
              />
            </div>
            <HotkeyField />
            <div className="two">
              <div className="field">
                <label htmlFor="pos">{t("settings.position")}</label>
                <select id="pos" className="select" value={settings.widget_position} onChange={(e) => set({ widget_position: e.target.value as AppSettings["widget_position"] })}>
                  <option value="platform_default">{t(snapshot.os === "macos" ? "settings.pos.topCenter" : "settings.pos.bottomRight")}</option>
                  <option value="last_position">{t("settings.pos.last")}</option>
                </select>
              </div>
              <div className="field">
                <span className="field-label">{t("settings.mic")}</span>
                <Segmented
                  label={t("settings.mic")}
                  value={settings.mic_mode}
                  onChange={(v) => set({ mic_mode: v })}
                  options={[{ value: "push_to_talk", label: t("settings.mic.hold") }, { value: "toggle", label: t("settings.mic.toggle") }]}
                />
              </div>
            </div>
            <div className="setting-row">
              <span>{t("settings.autostart")}</span>
              <Toggle checked={settings.launch_at_startup} label={t("settings.autostart")} onChange={(v) => set({ launch_at_startup: v })} />
            </div>
          </section>

          <section className="card settings-card" aria-labelledby="s-perf">
            <div className="setting-row">
              <h2 id="s-perf">{t("settings.performance")}</h2>
              <button type="button" className="btn btn-sm" disabled={!loaded.some((m) => isLoaded(m.status))} onClick={() => api.unloadAll().catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true))}>
                {t("models.unloadAll")}
              </button>
            </div>
            <div className="panel rows" style={{ padding: "0 14px" }}>
              {loaded.length === 0 && <div className="list-row small muted">{t("settings.noModels")}</div>}
              {loaded.map((m) => (
                <div key={m.model_id} className="list-row" style={{ padding: "12px 0", fontSize: 14.5 }}>
                  <Status tone={m.tone}><span style={{ color: "var(--ink)", fontSize: 14.5 }}>{m.display_name}</span></Status>
                  <span style={{ flex: 1 }} />
                  <span className="small muted">
                    {isLoaded(m.status)
                      ? `${t("settings.ram")}: ${m.memory_mb != null ? `${m.memory_mb} MB` : t("common.notMeasured")}`
                      : t("settings.notLoaded")}
                    {" · "}{t(`ttl.${m.ttl}`)}
                  </span>
                </div>
              ))}
            </div>
            <div className="three" role="group" aria-label={t("settings.usageLabel")}>
              <div className="panel stat usage-stat">
                <div className="k">{t("settings.modelsRam")}</div>
                <div className="v">{usage?.models_ram_mb != null ? formatMb(usage.models_ram_mb, lang) : usage && usage.running_models === 0 ? t("settings.noneRunning") : t("common.notMeasured")}</div>
              </div>
              <div className="panel stat usage-stat">
                <div className="k">{t("settings.gpu")}</div>
                <div className="v">{usage?.gpu ? `${formatMb(usage.gpu.used_mb, lang)} / ${formatMb(usage.gpu.total_mb, lang)}` : t("common.notMeasured")}</div>
                <div className="small muted">{usage?.gpu ? t("settings.gpuAllApps", { name: usage.gpu.name }) : t("settings.gpuNvidiaOnly")}</div>
              </div>
              <div className="panel stat usage-stat">
                <div className="k">{t("folders.statSize")}</div>
                <div className="v">{stats.data?.index_mb != null ? formatMb(stats.data.index_mb, lang) : "—"}</div>
              </div>
            </div>
            <span className="page-foot">{t("settings.measuredNote")}</span>
            <div className="field" role="group" aria-labelledby="s-llm">
              <span className="field-label" id="s-llm">{t("settings.llmRoles")}</span>
              <span className="small muted">{t("settings.llmRolesHint")}</span>
              {(["llm_plan", "llm_pick", "llm_caption"] as const).map((k) => (
                <div key={k} className="setting-row">
                  <span>{t(`settings.${k}`)}</span>
                  <Toggle checked={settings[k]} label={t(`settings.${k}`)} onChange={(v) => set({ [k]: v })} />
                </div>
              ))}
            </div>
          </section>

          <section className="card settings-card" aria-labelledby="s-privacy">
            <h2 id="s-privacy">{t("settings.privacy")}</h2>
            <div className="field">
              <label htmlFor="ret">{t("settings.retention")}</label>
              <select id="ret" className="select" value={settings.history_enabled ? settings.history_retention : "off"}
                onChange={(e) => e.target.value === "off" ? set({ history_enabled: false }) : set({ history_enabled: true, history_retention: e.target.value as AppSettings["history_retention"] })}>
                <option value="days30">{t("settings.ret.days30")}</option>
                <option value="days90">{t("settings.ret.days90")}</option>
                <option value="forever">{t("settings.ret.forever")}</option>
                <option value="off">{t("settings.ret.off")}</option>
              </select>
            </div>
            <span className="field-hint">{settings.history_enabled ? t("settings.retentionHint") : t("settings.historyOffHint")}</span>
            <div className="setting-row"><span>{t("settings.voiceStore")}</span><span className="small muted">{t("settings.never")}</span></div>
            <div className="setting-row"><span>{t("settings.network")}</span><Status tone="green">{t("settings.notUsed")}</Status></div>
            <div className="setting-row">
              <span>
                {t("settings.dataFolder")}
                <span className="field-hint" style={{ display: "block" }}>{t("settings.dataFolderHint")}</span>
              </span>
              <button type="button" className="btn btn-sm" onClick={showDataFolder} disabled={!isTauri()}>{t("settings.showFolder")}</button>
            </div>
            <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
              <button type="button" className="btn btn-sm" onClick={exportDiagnostics} disabled={!isTauri()} aria-describedby="diag-hint">{t("settings.exportDiag")}</button>
              <button type="button" className="btn btn-sm btn-danger" onClick={() => setWipe(true)}>{t("settings.wipe")}</button>
            </div>
            <span id="diag-hint" className="field-hint">{t("settings.diagHint")}</span>
          </section>

          <section className="card settings-card" aria-labelledby="s-about">
            <h2 id="s-about">{t("settings.about")}</h2>
            <div className="kv">
              <div><div className="k">{t("settings.version")}</div><div>{info?.app_version ?? "0.1.0"}</div></div>
              <div><div className="k">{t("settings.platform")}</div><div>{info ? `${info.os} · ${info.arch}` : t("settings.browserPreview")}</div></div>
              <div><div className="k">{t("settings.database")}</div><div>{health.data?.db_ok ? t("settings.dbOk") : t("settings.dbMissing")}</div></div>
              <div><div className="k">{t("settings.licenses")}</div><button type="button" className="btn-link" onClick={() => setLicenses(true)}>{t("settings.licensesLink")}</button></div>
            </div>
          </section>

        </div>
      </div>

      {licenses && (
        <Dialog title={t("settings.licensesTitle")} subtitle={t("settings.licensesIntro")} onClose={() => setLicenses(false)}>
          <ul className="license-list">
            {LICENSES.map((l) => (
              <li key={l.name}>
                <span className="name">{l.name}</span>
                <span className="small muted">{t(`settings.licenseKind.${l.kind}`)}</span>
                <span className="lic">{l.license}</span>
              </li>
            ))}
          </ul>
          <p className="field-hint">{t("settings.licensesNote")}</p>
          <div className="dialog-actions">
            <button type="button" className="btn" onClick={() => setLicenses(false)}>{t("common.close")}</button>
          </div>
        </Dialog>
      )}

      {wipe && (
        <Dialog title={t("settings.wipeTitle")} onClose={() => setWipe(false)} small>
          <p style={{ fontSize: 14.5, lineHeight: 1.5, color: "var(--ink-2)" }}>{t("settings.wipeBody")}</p>
          <div className="dialog-actions">
            <button type="button" className="btn" onClick={() => setWipe(false)}>{t("common.cancel")}</button>
            <button type="button" className="btn btn-danger-solid" onClick={wipeAll}>{t("common.delete")}</button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
