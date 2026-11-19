import { useEffect, useRef, useState } from "react";
import type { DownloadProgress } from "../../bindings/DownloadProgress";
import type { DownloadableModel } from "../../bindings/DownloadableModel";
import type { ModelCategory } from "../../bindings/ModelCategory";
import type { ValidationStep } from "../../bindings/ValidationStep";
import { Icon } from "../../ui/icons";
import { Dialog, Segmented } from "../../ui/primitives";
import { useI18n } from "../../i18n";
import { useData } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { isTauri, pickPath, toIpcError } from "../../ipc/bridge";
import { formatBytes } from "../../lib/format";
import { CATEGORIES } from "../../lib/health";
import { isMacOs, pathPlaceholder } from "../../lib/platform";

// Whisper (.bin), Gemma GGUF (.gguf), and Piper (.onnx) can be picked as files;
// folder paths can also be entered for multi-file validation.
const PICK_DIRECTORY: Record<ModelCategory, boolean> = { embedding: false, llm: false, stt: false, tts: false };

function stepDetailKey(detail: string, os: string): string {
  if (!isMacOs(os)) return detail;
  if (detail === "addModel.fail.runtime") return "addModel.fail.runtimeMac";
  if (detail === "addModel.fail.runtime.stt") return "addModel.fail.runtimeMac.stt";
  if (detail === "addModel.fail.runtime.tts") return "addModel.fail.runtimeMac.tts";
  if (detail === "addModel.fail.smoke") return "addModel.fail.smokeMac";
  return detail;
}

function StepIcon({ state }: { state: ValidationStep["state"] }) {
  if (state === "running") return <span className="spinner" aria-hidden="true" />;
  if (state === "ok") return <span className="step-icon ok"><Icon name="check" size={13} stroke={3} /></span>;
  if (state === "failed") return <span className="step-icon failed"><Icon name="close" size={12} stroke={3} /></span>;
  return <span className="step-icon pending" />;
}

export function AddModelDialog({ initial, onClose }: { initial: ModelCategory; onClose: () => void }) {
  const { t, lang } = useI18n();
  const { api } = useData();
  const { snapshot } = useSettings();
  const toast = useToast();
  const [category, setCategory] = useState<ModelCategory>(initial);
  const [mode, setMode] = useState<"local" | "download">("local");
  const [path, setPath] = useState("");
  const [steps, setSteps] = useState<ValidationStep[]>([]);
  const [result, setResult] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);

  const [catalog, setCatalog] = useState<DownloadableModel[]>([]);
  const [presetId, setPresetId] = useState("");
  const [destDir, setDestDir] = useState("");
  const [consent, setConsent] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [dlProgress, setDlProgress] = useState<DownloadProgress | null>(null);
  const [dlErrorKey, setDlErrorKey] = useState<string | null>(null);

  const abort = useRef<AbortController | null>(null);
  const dlAbort = useRef<AbortController | null>(null);

  useEffect(() => {
    let dead = false;
    api.listDownloadableModels().then((list) => {
      if (!dead) setCatalog(list);
    }).catch(() => {});
    return () => { dead = true; };
  }, [api]);

  const presets = catalog.filter((m) => m.category === category);
  const selectedPreset = presets.find((p) => p.preset_id === presetId) ?? presets[0] ?? null;

  useEffect(() => {
    if (selectedPreset && mode === "download") {
      setDestDir(selectedPreset.default_dir);
    }
  }, [selectedPreset?.preset_id, selectedPreset?.default_dir, mode]);

  const validate = async (p: string, c: ModelCategory) => {
    abort.current?.abort();
    const ctl = new AbortController();
    abort.current = ctl;
    setResult(null);
    setSteps([]);
    try {
      const ok = await api.validateModel({ category: c, path: p }, setSteps, ctl.signal);
      if (!ctl.signal.aborted) setResult(ok);
    } catch (e) {
      const err = toIpcError(e);
      if (err.code !== "USER_CANCELLED") toast(t(`error.${err.code}.title`), true);
      setResult(err.code === "USER_CANCELLED" ? null : false);
      if (err.code === "USER_CANCELLED") setSteps([]);
    }
  };

  useEffect(() => () => {
    abort.current?.abort();
    dlAbort.current?.abort();
  }, []);

  const browse = async () => {
    const p = await pickPath(PICK_DIRECTORY[category], t("addModel.pickTitle"));
    if (p) {
      setPath(p);
      validate(p, category);
    }
  };

  const browseDest = async () => {
    const p = await pickPath(true, t("addModel.downloadPickDir"), destDir || undefined);
    if (p) setDestDir(p);
  };

  const resetTasks = () => {
    abort.current?.abort();
    dlAbort.current?.abort();
    setDownloading(false);
    setDlProgress(null);
    setDlErrorKey(null);
    setConsent(false);
  };

  const changeCategory = (c: ModelCategory) => {
    resetTasks();
    setCategory(c);
    setMode("local");
    setPresetId("");
    setPath("");
    setSteps([]);
    setResult(null);
  };

  const changeMode = (next: "local" | "download") => {
    resetTasks();
    setMode(next);
  };

  const startDownload = async () => {
    if (!selectedPreset || !consent || !destDir.trim()) return;
    dlAbort.current?.abort();
    const ctl = new AbortController();
    dlAbort.current = ctl;
    setDownloading(true);
    setDlErrorKey(null);
    setDlProgress({
      preset_id: selectedPreset.preset_id,
      phase: "downloading",
      file_index: 1,
      file_count: selectedPreset.files.length,
      file_name: selectedPreset.files[0]?.filename ?? "",
      bytes_done: 0,
      bytes_total: selectedPreset.total_bytes,
      destination_path: null,
      error_key: null,
    });
    try {
      const finalDir = await api.downloadModel(
        { preset_id: selectedPreset.preset_id, destination_dir: destDir.trim(), consent: true },
        setDlProgress,
        ctl.signal,
      );
      if (ctl.signal.aborted) return;
      setDownloading(false);
      setDlProgress(null);
      setConsent(false);
      setMode("local");
      setPath(finalDir);
      toast(t("addModel.downloadDone"));
      await validate(finalDir, category);
    } catch (e) {
      if (ctl.signal.aborted) {
        setDownloading(false);
        setDlProgress(null);
        return;
      }
      const err = toIpcError(e);
      setDownloading(false);
      if (err.code !== "USER_CANCELLED") {
        const key = err.detail.startsWith("addModel.downloadErr.") ? err.detail : `error.${err.code}.body`;
        setDlErrorKey(key);
        toast(t(`error.${err.code}.title`), true);
      } else {
        setDlProgress(null);
      }
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const m = await api.registerModel({ category, path });
      toast(t("addModel.savedToast", { name: m.display_name }));
      onClose();
    } catch (e) {
      toast(t(`error.${toIpcError(e).code}.title`), true);
      setSaving(false);
    }
  };

  const running = steps.some((s) => s.state === "running");
  const failed = steps.find((s) => s.state === "failed");
  const hintKey = `addModel.supported.${category}`;
  const dlPct = dlProgress && dlProgress.bytes_total > 0
    ? Math.min(100, Math.round((dlProgress.bytes_done / dlProgress.bytes_total) * 100))
    : 0;

  return (
    <Dialog title={t("addModel.title")} onClose={onClose}>
      <div className="field">
        <span className="field-label" id="cat-label">{t("addModel.type")}</span>
        <Segmented
          large
          label={t("addModel.type")}
          value={category}
          onChange={changeCategory}
          options={CATEGORIES.map((c) => ({ value: c, label: t(`category.${c}`) }))}
        />
      </div>

      {presets.length > 0 && (
        <div className="field">
          <span className="field-label">{t("addModel.source")}</span>
          <Segmented
            label={t("addModel.source")}
            value={mode}
            onChange={changeMode}
            options={[
              { value: "local", label: t("addModel.sourceLocal") },
              { value: "download", label: t("addModel.sourceDownload") },
            ]}
          />
        </div>
      )}

      {mode === "local" || !selectedPreset ? (
        <>
          <div className="field">
            <label htmlFor="model-path">{t("addModel.path")}</label>
            <div style={{ display: "flex", gap: 10 }}>
              <input
                id="model-path"
                className="input"
                style={{ flex: 1 }}
                value={path}
                placeholder={pathPlaceholder("model", snapshot.os)}
                onChange={(e) => { setPath(e.target.value); setSteps([]); setResult(null); }}
                onKeyDown={(e) => e.key === "Enter" && path.trim() && validate(path, category)}
              />
              {isTauri() && <button type="button" className="btn" onClick={browse}>{t("addModel.browse")}</button>}
              {path.trim() && !running && result === null && (
                <button type="button" className="btn" onClick={() => validate(path, category)}>{t("addModel.validate")}</button>
              )}
            </div>
            <span className="field-hint">{t(hintKey)} {t("addModel.inPlace")}</span>
          </div>

          {steps.length > 0 && (
            <div className="steps" aria-live="polite">
              {steps.map((s) => (
                <div className="step" key={s.id}>
                  <StepIcon state={s.state} />
                  <span className="label">
                    {t(s.label_key)}
                    {s.detail && <span className={`small ${s.state === "failed" ? "" : "muted"}`} style={{ display: "block", color: s.state === "failed" ? "var(--err-text)" : undefined }}>{t(stepDetailKey(s.detail, snapshot.os))}</span>}
                  </span>
                  {s.state === "ok" && <span className="small" style={{ color: "var(--ok-text)" }}>{t("addModel.ok")}</span>}
                  {s.state === "running" && (
                    <button type="button" className="btn btn-xs" onClick={() => abort.current?.abort()}>{t("common.cancel")}</button>
                  )}
                </div>
              ))}
            </div>
          )}

          {failed ? (
            <div className="note err"><Icon name="warn" size={18} /><span>{t("addModel.failedNote")}</span></div>
          ) : (
            <div className="note"><Icon name="info" size={18} /><span>{t("addModel.note")}</span></div>
          )}

          <div className="dialog-actions">
            <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
            <button type="button" className="btn btn-primary" disabled={result !== true || saving} onClick={save}>{t("common.save")}</button>
          </div>
        </>
      ) : (
        <>
          <div role="radiogroup" aria-label={t("addModel.sourceDownload")} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {presets.map((p) => (
              <label key={p.preset_id} className="radio-card">
                <input
                  type="radio"
                  name="dl-preset"
                  checked={selectedPreset.preset_id === p.preset_id}
                  disabled={downloading}
                  onChange={() => { setPresetId(p.preset_id); setConsent(false); setDlErrorKey(null); }}
                />
                <span style={{ flex: 1 }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <strong>{p.display_name}</strong>
                    {p.recommended && <span className="badge">{t("addModel.downloadRecommended")}</span>}
                  </span>
                  <span className="muted small">{formatBytes(p.total_bytes, lang)} · {p.capabilities.join(" + ")}</span>
                </span>
              </label>
            ))}
          </div>

          <div className="dl-meta">
            <div className="dl-meta-row">
              <span className="muted">{t("addModel.downloadOrigin")}</span>
              <span>{selectedPreset.origin_host}/{selectedPreset.repo} ({selectedPreset.revision.slice(0, 7)})</span>
            </div>
            <div className="dl-meta-row">
              <span className="muted">{t("addModel.downloadLicense")}</span>
              <span>{selectedPreset.license_name}</span>
            </div>
            <div className="dl-meta-row">
              <span className="muted">{t("addModel.downloadSize")}</span>
              <span>{formatBytes(selectedPreset.total_bytes, lang)}</span>
            </div>
            <ul className="dl-files" aria-label={t("addModel.downloadFiles")}>
              {selectedPreset.files.map((f) => (
                <li key={f.filename}>
                  <span>{f.filename}</span>
                  <span>{formatBytes(f.size_bytes, lang)}</span>
                </li>
              ))}
            </ul>
          </div>

          <div className="field">
            <label htmlFor="dl-dest">{t("addModel.downloadDest")}</label>
            <div style={{ display: "flex", gap: 10 }}>
              <input
                id="dl-dest"
                className="input"
                style={{ flex: 1 }}
                value={destDir}
                disabled={downloading}
                onChange={(e) => setDestDir(e.target.value)}
              />
              {isTauri() && <button type="button" className="btn" disabled={downloading} onClick={browseDest}>{t("addModel.browse")}</button>}
            </div>
          </div>

          <label className="check">
            <input
              type="checkbox"
              checked={consent}
              disabled={downloading}
              onChange={(e) => setConsent(e.target.checked)}
            />
            <span>{t("addModel.downloadConsent", { host: selectedPreset.origin_host, size: formatBytes(selectedPreset.total_bytes, lang), license: selectedPreset.license_name })}</span>
          </label>

          {downloading && dlProgress && (
            <div className="dl-progress" aria-live="polite">
              <div className="dl-meta-row">
                <span>
                  {t(`addModel.downloadPhase.${dlProgress.phase}`)} · {dlProgress.file_index}/{dlProgress.file_count} · {dlProgress.file_name}
                </span>
                <span>{formatBytes(dlProgress.bytes_done, lang)} / {formatBytes(dlProgress.bytes_total, lang)}</span>
              </div>
              <div
                className="progress"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={dlPct}
                aria-label={selectedPreset.display_name}
              >
                <span style={{ width: `${dlPct}%` }} />
              </div>
            </div>
          )}

          {dlErrorKey && (
            <div className="note err"><Icon name="warn" size={18} /><span>{t(dlErrorKey)}</span></div>
          )}

          <div className="dialog-actions">
            {downloading ? (
              <button type="button" className="btn" onClick={() => dlAbort.current?.abort()}>{t("common.cancel")}</button>
            ) : (
              <>
                <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
                <button type="button" className="btn btn-primary" disabled={!consent || !destDir.trim()} onClick={startDownload}>{t("addModel.downloadStart")}</button>
              </>
            )}
          </div>
        </>
      )}
    </Dialog>
  );
}
