import { useEffect, useRef, useState } from "react";
import type { ModelCategory } from "../../bindings/ModelCategory";
import type { ValidationStep } from "../../bindings/ValidationStep";
import { Icon } from "../../ui/icons";
import { Dialog, Segmented } from "../../ui/primitives";
import { useT } from "../../i18n";
import { useData } from "../../state/data";
import { useToast } from "../../state/toast";
import { isTauri, pickPath, toIpcError } from "../../ipc/bridge";
import { CATEGORIES } from "../../lib/health";

// Browser preview has no file picker; these sample paths show the flow.
const SAMPLE: Record<ModelCategory, string> = {
  embedding: "C:/Models/embeddinggemma-2",
  llm: "C:/Models/gemma-4-e4b-Q4_K_M.gguf",
  stt: "C:/Models/whisper-base",
  tts: "C:/Models/tr_TR-voice.onnx",
};

function StepIcon({ state }: { state: ValidationStep["state"] }) {
  if (state === "running") return <span className="spinner" aria-hidden="true" />;
  if (state === "ok") return <span className="step-icon ok"><Icon name="check" size={13} stroke={3} /></span>;
  if (state === "failed") return <span className="step-icon failed"><Icon name="close" size={12} stroke={3} /></span>;
  return <span className="step-icon pending" />;
}

export function AddModelDialog({ initial, onClose }: { initial: ModelCategory; onClose: () => void }) {
  const t = useT();
  const { api } = useData();
  const toast = useToast();
  const [category, setCategory] = useState<ModelCategory>(initial);
  const [path, setPath] = useState("");
  const [steps, setSteps] = useState<ValidationStep[]>([]);
  const [result, setResult] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const abort = useRef<AbortController | null>(null);

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

  useEffect(() => () => abort.current?.abort(), []);

  const browse = async () => {
    const p = isTauri() ? await pickPath(category === "embedding" || category === "stt", t("addModel.pickTitle")) : SAMPLE[category];
    if (p) {
      setPath(p);
      validate(p, category);
    }
  };

  const changeCategory = (c: ModelCategory) => {
    abort.current?.abort();
    setCategory(c);
    setPath("");
    setSteps([]);
    setResult(null);
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

      <div className="field">
        <label htmlFor="model-path">{t("addModel.path")}</label>
        <div style={{ display: "flex", gap: 10 }}>
          <input
            id="model-path"
            className="input"
            style={{ flex: 1 }}
            value={path}
            placeholder={SAMPLE[category]}
            onChange={(e) => { setPath(e.target.value); setSteps([]); setResult(null); }}
            onKeyDown={(e) => e.key === "Enter" && path.trim() && validate(path, category)}
          />
          <button type="button" className="btn" onClick={browse}>{t("addModel.browse")}</button>
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
                {s.detail && <span className={`small ${s.state === "failed" ? "" : "muted"}`} style={{ display: "block", color: s.state === "failed" ? "var(--err-text)" : undefined }}>{t(s.detail)}</span>}
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
    </Dialog>
  );
}
