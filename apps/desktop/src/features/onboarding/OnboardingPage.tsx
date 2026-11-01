import { useState, type ReactNode } from "react";
import { Icon } from "../../ui/icons";
import { useT } from "../../i18n";
import { useData, useResource } from "../../state/data";
import { useSettings } from "../../state/settings";
import { useToast } from "../../state/toast";
import { isLoaded } from "../../lib/health";
import { toIpcError } from "../../ipc/bridge";
import { navigate } from "../../shell/router";
import { AddModelDialog } from "../models/AddModelDialog";
import { AddFolderDialog } from "../folders/FolderDialogs";

function Step({ n, state, title, badge, desc, action }: {
  n: number; state: "todo" | "required" | "done"; title: string; badge?: ReactNode; desc: ReactNode; action: ReactNode;
}) {
  return (
    <li className={`card wstep${state === "required" ? " err" : ""}`}>
      {state === "done" ? (
        <span className="num ok" role="img" aria-label={`${n}`}><Icon name="check" size={16} stroke={3} /></span>
      ) : (
        <span className={`num${state === "required" ? " err" : ""}`}>{n}</span>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="title">{title}{badge}</div>
        <div className="desc">{desc}</div>
      </div>
      {action}
    </li>
  );
}

/** First launch with zero models (J01, A10). Keeps settings/folders usable. */
export function OnboardingPage() {
  const t = useT();
  const { api } = useData();
  const { settings } = useSettings();
  const toast = useToast();
  const models = useResource((a) => a.listModels());
  const scopes = useResource((a) => a.listScopes().catch(() => []));
  const health = useResource((a) => a.health());
  const [dialog, setDialog] = useState<"model" | "folder" | null>(null);

  const emb = models.data?.find((m) => m.category === "embedding");
  const optional = (models.data ?? []).filter((m) => m.category !== "embedding").length;
  const folders = (scopes.data ?? []).length;
  const ready = !!health.data?.semantic_search_enabled;

  const load = () => emb && api.loadModel(emb.model_id).catch((e) => toast(t(`error.${toIpcError(e).code}.title`), true));

  return (
    <main className="welcome">
      <div className="welcome-inner">
        <div>
          <h1>{t("welcome.title")}</h1>
          <p className="lead">{t("welcome.lead")}</p>
        </div>
        <ol style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <Step
            n={1}
            state={emb && emb.tone !== "red" ? "done" : "required"}
            title="EmbeddingGemma 2"
            badge={!emb ? <span className="badge badge-err">{t("welcome.requiredMissing")}</span> : <span className="badge">{t(`status.${emb.status}`)}</span>}
            desc={emb ? (isLoaded(emb.status) ? t("welcome.embReady") : t("welcome.embLoad")) : t("welcome.embDesc")}
            action={
              !emb ? <button type="button" className="btn btn-primary" onClick={() => setDialog("model")}>{t("welcome.chooseFolder")}</button>
              : !isLoaded(emb.status) ? <button type="button" className="btn btn-primary" disabled={emb.status === "LOADING"} onClick={load}>{t("models.load")}</button>
              : null
            }
          />
          <Step
            n={2}
            state={optional ? "done" : "todo"}
            title={t("welcome.optionalTitle")}
            badge={<span className="badge">{t("status.OPTIONAL")}</span>}
            desc={t("welcome.optionalDesc")}
            action={<a href="#/models?add" className="btn">{t("welcome.addNow")}</a>}
          />
          <Step
            n={3}
            state={folders ? "done" : "todo"}
            title={t("welcome.folderTitle")}
            desc={folders ? t("welcome.folderDone", { n: folders }) : t("welcome.folderDesc")}
            action={<button type="button" className="btn" onClick={() => setDialog("folder")}>{t("folders.add")}</button>}
          />
          <Step
            n={4}
            state="done"
            title={t("welcome.hotkeyTitle")}
            desc={<>{t("welcome.hotkeyDesc")} <kbd>{settings.hotkey.replace(/\+/g, " + ")}</kbd></>}
            action={<a href="#/settings" className="btn">{t("settings.change")}</a>}
          />
        </ol>
        <div className="welcome-foot">
          <span className="small muted" style={{ maxWidth: 420 }}>{t("welcome.foot")}</span>
          <div className="page-actions">
            <button type="button" className="btn btn-ghost" onClick={() => navigate("search")}>{t("welcome.skip")}</button>
            <button type="button" className="btn btn-primary" style={{ height: 48, padding: "0 28px", fontSize: 16 }} disabled={!ready} onClick={() => navigate("search")}>
              {t("welcome.start")}
            </button>
          </div>
        </div>
      </div>
      {dialog === "model" && <AddModelDialog initial="embedding" onClose={() => setDialog(null)} />}
      {dialog === "folder" && <AddFolderDialog onClose={() => setDialog(null)} />}
    </main>
  );
}
