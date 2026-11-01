import { useEffect, useRef } from "react";
import type { HistoryTurn } from "../../bindings/HistoryTurn";
import type { IpcError } from "../../bindings/IpcError";
import { Icon } from "../../ui/icons";
import { useI18n } from "../../i18n";
import { clockTime, relativeTime } from "../../lib/format";
import { ResultCard } from "./ResultCard";

type Props = {
  turns: HistoryTurn[];
  pending: string | null;
  error: IpcError | null;
  onChoice: (id: string, label: string) => void;
  onOpen: (ref: string) => void;
  onReveal: (ref: string) => void;
  onPreview: (ref: string) => void;
  onSuggest: (kind: "filenames" | "ocr" | "folder") => void;
};

function AiAvatar() {
  return (
    <span className="ai-avatar" aria-hidden="true">
      <Icon name="spark" size={20} color="var(--accent)" />
    </span>
  );
}

function UserMsg({ text, at }: { text: string; at?: string }) {
  const { lang } = useI18n();
  return (
    <div className="msg-user">
      <div className="col">
        <div className="bubble-user">{text}</div>
        {at && <span className="msg-time">{clockTime(at, lang)}</span>}
      </div>
      <span className="avatar" aria-hidden="true"><Icon name="user" size={18} /></span>
    </div>
  );
}

function AssistantTurn({ turn, isLast, ...p }: { turn: HistoryTurn; isLast: boolean } & Omit<Props, "turns" | "pending" | "error">) {
  const { t, lang } = useI18n();
  const n = turn.count ?? 0;

  if (turn.kind === "clarify") {
    const projects = turn.choices.filter((c) => c.id !== "all").length;
    return (
      <div className="msg-ai">
        <AiAvatar />
        <div className="ai-col">
          <div className="bubble-ai">
            <div className="lead">{t("search.clarify", { n, projects })}</div>
            <div className="choices" role="group" aria-label={t("search.choicesLabel")}>
              {turn.choices.map((c, i) => (
                <button
                  key={c.id}
                  type="button"
                  className="chip chip-choice"
                  disabled={!isLast}
                  onClick={() => p.onChoice(c.id, c.id === "all" ? t("search.showAll") : c.label)}
                >
                  {c.id === "all" ? t("search.showAll") : c.label}
                  {c.id !== "all" && <span className="count">· {c.count}</span>}
                  {isLast && <span className="chip-key" aria-hidden="true">{i + 1}</span>}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (turn.kind === "not_found") {
    return (
      <div className="msg-ai">
        <AiAvatar />
        <div className="ai-col">
          <div className="bubble-ai notfound">
            <div className="lead">
              <strong>{t("search.notFound")}</strong> <span className="muted">{t("search.notFoundWhy")}</span>
            </div>
            {isLast && (
              <div className="choices">
                <button type="button" className="chip" onClick={() => p.onSuggest("filenames")}>{t("search.tryFilenames")}</button>
                <button type="button" className="chip" onClick={() => p.onSuggest("ocr")}>{t("search.tryOcr")}</button>
                <button type="button" className="chip" onClick={() => p.onSuggest("folder")}>{t("folders.add")}</button>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  const prev = turn.results[0]?.match_reason === "previous_result";
  return (
    <div className="msg-ai">
      <AiAvatar />
      <div className="ai-col">
        <div className="bubble-ai">
          <div>
            <div className="lead">
              {prev ? (
                <>{t("search.foundPrevA")} <strong>{t("search.foundPrevB")}</strong> {t("search.foundPrevC", { when: relativeTime(turn.results[0].modified_at, lang) })}</>
              ) : (
                <>{t("search.foundA")} <strong>{t("search.foundB", { n })}</strong>{t("search.foundC")}</>
              )}
            </div>
            <div className="sub">{t("search.bestMatches", { n: turn.results.length })}</div>
          </div>
        </div>
        {turn.results.map((r) => (
          <ResultCard key={r.result_ref} r={r} opened={turn.opened} onOpen={p.onOpen} onReveal={p.onReveal} onPreview={p.onPreview} />
        ))}
      </div>
    </div>
  );
}

export function Thread(props: Props) {
  const { turns, pending, error } = props;
  const { t } = useI18n();
  const endRef = useRef<HTMLDivElement>(null);
  const lastAi = [...turns].reverse().find((x) => x.role === "assistant");

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end", behavior: "smooth" });
  }, [turns.length, pending]);

  // 1–9 pick a clarification choice when not typing (docs/24 §5).
  useEffect(() => {
    if (lastAi?.kind !== "clarify") return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT" || e.ctrlKey || e.metaKey || e.altKey) return;
      const i = Number(e.key) - 1;
      const c = lastAi.choices[i];
      if (c) props.onChoice(c.id, c.id === "all" ? t("search.showAll") : c.label);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [lastAi, props, t]);

  // ↑/↓ move between result cards.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const cards = Array.from(document.querySelectorAll<HTMLElement>("[data-result]"));
    const i = cards.indexOf(document.activeElement as HTMLElement);
    if (i < 0 && !cards.length) return;
    e.preventDefault();
    const next = e.key === "ArrowDown" ? Math.min(cards.length - 1, i + 1) : Math.max(0, i - 1);
    cards[next]?.focus();
  };

  return (
    <div className="thread" onKeyDown={onKeyDown} aria-live="polite">
      {turns.map((turn) =>
        turn.role === "user" ? (
          <UserMsg key={turn.turn_id} text={turn.text ?? ""} at={turn.at} />
        ) : (
          <AssistantTurn key={turn.turn_id} turn={turn} isLast={turn === lastAi && !pending} {...props} />
        ),
      )}
      {pending !== null && (
        <>
          {pending && <UserMsg text={pending} />}
          <div className="msg-ai">
            <AiAvatar />
            <div className="typing" role="status" aria-label={t("search.searching")}><span /><span /><span /></div>
          </div>
        </>
      )}
      {error && (
        <div className="msg-ai">
          <AiAvatar />
          <div className="bubble-ai notfound" role="alert">
            <div className="lead"><strong>{t(`error.${error.code}.title`)}</strong></div>
            <div className="sub">{t(`error.${error.code}.body`)}</div>
          </div>
        </div>
      )}
      <div ref={endRef} />
    </div>
  );
}
