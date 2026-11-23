import { useEffect, useMemo, useRef, useState } from "react";
import type { HistoryTurn } from "../../bindings/HistoryTurn";
import type { IpcError } from "../../bindings/IpcError";
import type { Understood } from "../../bindings/Understood";
import type { Summary } from "../../bindings/Summary";
import type { ResultCard as Card } from "../../bindings/ResultCard";
import { Icon } from "../../ui/icons";
import { useI18n } from "../../i18n";
import { clockTime, relativeTime } from "../../lib/format";
import { ResultRow } from "./ResultCard";
import type { SpeechController } from "../voice/useSpeech";

type Props = {
  turns: HistoryTurn[];
  pending: string | null;
  error: IpcError | null;
  ttsAvailable?: boolean;
  speech?: SpeechController;
  onChoice: (id: string, label: string) => void;
  onOpen: (ref: string) => void;
  onReveal: (ref: string) => void;
  onPreview: (ref: string) => void;
  onSuggest: (kind: "filenames" | "ocr" | "folder") => void;
};

/** `[2]`, `[1, 3]` citation markers. */
const CITE = /\s*\[[\d,\s]+\]/g;

/** Text without citation markers, for display where markers can't be linked. */
export function stripCites(s: string): string {
  return s.replace(CITE, "").replace(/\s+([.,;:!?])/g, "$1").trim();
}

function norm(s: string): string {
  return stripCites(s).toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

/** True when the summary only repeats the direct answer. */
export function sameText(a: string, b: string): boolean {
  const x = norm(a);
  const y = norm(b);
  return !!x && !!y && (x === y || x.includes(y) || y.includes(x));
}

/** One exchange: the user's question and the reply to it. */
type Exchange = { id: string; question: HistoryTurn | null; reply: HistoryTurn | null };

function group(turns: HistoryTurn[]): Exchange[] {
  const out: Exchange[] = [];
  for (const turn of turns) {
    if (turn.role === "user") out.push({ id: turn.turn_id, question: turn, reply: null });
    else {
      const last = out[out.length - 1];
      if (last && !last.reply) last.reply = turn;
      else out.push({ id: turn.turn_id, question: null, reply: turn });
    }
  }
  return out;
}

const anchor = (id: string) => `qa-${id}`;

/** What the request was read as: filters and words added by the LLM. */
export function UnderstoodLine({ u }: { u: Understood | null }) {
  const { t } = useI18n();
  if (!u) return null;
  const parts: string[] = [];
  if (u.file_type !== "any") parts.push(t(`understood.type.${u.file_type}`));
  if (u.date !== "any") parts.push(t(`understood.date.${u.date}`));
  if (!parts.length && !u.exclude.length && !u.added_terms.length) return null;
  return (
    <div className="understood small muted" data-testid="understood">
      <span>{t("understood.label")}</span>
      {parts.map((p) => <span key={p} className="pill">{p}</span>)}
      {u.exclude.map((w) => <span key={`x-${w}`} className="pill">{t("understood.without", { w })}</span>)}
      {u.added_terms.length > 0 && (
        <span title={u.by_llm ? t("understood.byLlm") : undefined}>{t("understood.also", { words: u.added_terms.join(", ") })}</span>
      )}
    </div>
  );
}

/** LLM summary; each [k] opens the preview of the cited result. */
export function SummaryBlock({ s, results, onPreview, titled = true }: { s: Summary; results: Card[]; onPreview: (ref: string) => void; titled?: boolean }) {
  const { t } = useI18n();
  const parts = s.text.split(/(\[[\d,\s]+\])/g);
  return (
    <div className={`summary${titled ? "" : " plain"}`} data-testid="summary">
      {titled && <div className="summary-head">{t("search.summaryTitle")}</div>}
      <p>
        {parts.map((p, i) => {
          const m = p.match(/^\[([\d,\s]+)\]$/);
          if (!m) return <span key={i}>{p}</span>;
          return (
            <span key={i} className="cites">
              {m[1].split(",").map((n) => {
                const ref = s.sources[Number(n.trim()) - 1];
                const card = results.find((r) => r.result_ref === ref);
                if (!ref || !card) return null;
                return (
                  <button
                    key={n}
                    type="button"
                    className="cite"
                    title={card.title}
                    aria-label={t("search.citeLabel", { n: n.trim(), file: card.title })}
                    onClick={() => onPreview(ref)}
                  >
                    {n.trim()}
                  </button>
                );
              })}
            </span>
          );
        })}
      </p>
    </div>
  );
}

function Question({ turn }: { turn: HistoryTurn }) {
  const { lang } = useI18n();
  return (
    <div className="qa-q">
      <h2 className="qa-q-text">{turn.text ?? ""}</h2>
      {turn.at && <time className="qa-time" dateTime={turn.at}>{clockTime(turn.at, lang)}</time>}
    </div>
  );
}

function ReadAloud({ turn, speech }: { turn: HistoryTurn; speech: SpeechController }) {
  const { t } = useI18n();
  const spoken = [turn.answer?.text, turn.summary?.text].filter(Boolean).map((x) => stripCites(x!)).join(". ");
  const speaking = speech.speakingId === turn.turn_id;
  return (
    <button
      type="button"
      className="btn btn-xs btn-ghost"
      disabled={speech.loadingId === turn.turn_id}
      onClick={() => speech.speak(turn.turn_id, spoken)}
    >
      <Icon name={speaking ? "stop" : "speakerWaves"} size={15} />
      {speech.loadingId === turn.turn_id ? t("voice.reading") : speaking ? t("voice.stopReading") : t("voice.readAloud")}
    </button>
  );
}

function Reply({ turn, isLast, ...p }: { turn: HistoryTurn; isLast: boolean } & Omit<Props, "turns" | "pending" | "error">) {
  const { t, lang } = useI18n();
  const n = turn.count ?? 0;

  if (turn.kind === "clarify") {
    const projects = turn.choices.filter((c) => c.id !== "all").length;
    return (
      <div className="qa-answer">
        <p className="qa-lead">{turn.question ?? t("search.clarify", { n, projects })}</p>
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
              {c.id !== "all" && c.count > 0 && <span className="count">· {c.count}</span>}
              {isLast && <span className="chip-key" aria-hidden="true">{i + 1}</span>}
            </button>
          ))}
        </div>
      </div>
    );
  }

  if (turn.kind === "not_found") {
    return (
      <div className="qa-answer empty">
        <p className="qa-lead"><strong>{t("search.notFound")}</strong></p>
        <p className="qa-sub">{t("search.notFoundWhy")}</p>
        {turn.notice && <p className="qa-sub notice">{t(`search.notice.${turn.notice}`)}</p>}
        <UnderstoodLine u={turn.understood} />
        {isLast && (
          <div className="choices">
            <button type="button" className="chip" onClick={() => p.onSuggest("filenames")}>{t("search.tryFilenames")}</button>
            <button type="button" className="chip" onClick={() => p.onSuggest("ocr")}>{t("search.tryOcr")}</button>
            <button type="button" className="chip" onClick={() => p.onSuggest("folder")}>{t("folders.add")}</button>
          </div>
        )}
      </div>
    );
  }

  const prev = turn.results[0]?.match_reason === "previous_result";
  // LLM picks first; the other matches stay one click away, never hidden.
  const picked = turn.picked_by_llm ? Math.min(turn.picked_count ?? turn.results.length, turn.results.length) : 0;
  const top = picked > 0 ? turn.results.slice(0, picked) : turn.results;
  const others = picked > 0 ? turn.results.slice(picked) : [];
  const answer = turn.answer;
  const answerFile = answer ? turn.results.find((r) => r.result_ref === answer.result_ref) : undefined;
  // The summary is shown only when it says more than the direct answer.
  const summary = turn.summary && !(answer && sameText(answer.text, turn.summary.text)) ? turn.summary : null;
  const hasText = !!answer || !!summary;

  return (
    <>
      <div className="qa-answer">
        <div className="qa-meta">
          <span>{prev ? t("search.prevShort", { when: relativeTime(turn.results[0].modified_at, lang) }) : t("search.foundB", { n })}</span>
          {picked > 0 && <span className="qa-meta-llm"><Icon name="spark" size={13} color="var(--accent)" />{t("search.pickedShort", { n: picked })}</span>}
        </div>
        {answer && (
          <div className="qa-direct" data-testid="direct-answer">
            <p className="qa-text">{stripCites(answer.text)}</p>
            {answerFile && (
              <button type="button" className="qa-source" onClick={() => p.onPreview(answerFile.result_ref)} title={t("result.preview")}>
                <Icon name="fileText" size={14} />{t("search.source")}: {answerFile.title}
              </button>
            )}
          </div>
        )}
        {summary && <SummaryBlock s={summary} results={turn.results} onPreview={p.onPreview} titled={!!answer} />}
        {!hasText && (
          <p className="qa-text muted-text">
            {prev ? t("search.foundPrevShort") : picked > 0 ? t("search.picked", { n: picked }) : t("search.bestMatches", { n: turn.results.length })}
          </p>
        )}
        {turn.notice && <p className="qa-sub notice">{t(`search.notice.${turn.notice}`)}</p>}
        {(turn.understood || (p.ttsAvailable && p.speech && hasText)) && (
          <div className="qa-foot">
            <UnderstoodLine u={turn.understood} />
            {p.ttsAvailable && p.speech && hasText && <ReadAloud turn={turn} speech={p.speech} />}
          </div>
        )}
      </div>
      <div className="qa-results">
        {top.map((r) => (
          <ResultRow key={r.result_ref} r={r} opened={turn.opened} onOpen={p.onOpen} onReveal={p.onReveal} onPreview={p.onPreview} />
        ))}
        {others.length > 0 && (
          <details className="other-results" data-testid="other-results">
            <summary>{t("search.otherResults", { n: others.length })}</summary>
            {others.map((r) => (
              <ResultRow key={r.result_ref} r={r} opened={turn.opened} onOpen={p.onOpen} onReveal={p.onReveal} onPreview={p.onPreview} />
            ))}
          </details>
        )}
      </div>
    </>
  );
}

/** Sticky list of the questions in this search; click jumps to one. */
export function QuestionNav({ turns }: { turns: HistoryTurn[] }) {
  const { t } = useI18n();
  const qs = useMemo(() => turns.filter((x) => x.role === "user"), [turns]);
  const [active, setActive] = useState<string | null>(null);

  useEffect(() => {
    if (qs.length < 2 || typeof IntersectionObserver === "undefined") return;
    const els = qs.map((q) => document.getElementById(anchor(q.turn_id))).filter((e): e is HTMLElement => !!e);
    const io = new IntersectionObserver(
      (entries) => {
        const vis = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (vis[0]) setActive(vis[0].target.id.slice(3));
      },
      { rootMargin: "-80px 0px -55% 0px" },
    );
    els.forEach((e) => io.observe(e));
    return () => io.disconnect();
  }, [qs]);

  if (qs.length < 2) return null;
  const current = active ?? qs[qs.length - 1].turn_id;
  return (
    <nav className="qa-nav" aria-label={t("search.qaNav")}>
      {qs.map((q, i) => (
        <button
          key={q.turn_id}
          type="button"
          className="qa-nav-item"
          aria-current={q.turn_id === current ? "true" : undefined}
          title={q.text ?? ""}
          onClick={() => document.getElementById(anchor(q.turn_id))?.scrollIntoView?.({ block: "start", behavior: "smooth" })}
        >
          <span className="n">{i + 1}</span>
          <span className="q">{q.text}</span>
        </button>
      ))}
    </nav>
  );
}

function Typing() {
  const { t } = useI18n();
  return <div className="typing" role="status" aria-label={t("search.searching")}><span /><span /><span /></div>;
}

export function Thread(props: Props) {
  const { turns, pending, error } = props;
  const { t } = useI18n();
  const endRef = useRef<HTMLDivElement>(null);
  const exchanges = useMemo(() => group(turns), [turns]);
  const lastAi = [...turns].reverse().find((x) => x.role === "assistant");

  // Bring the newest question to the top of the view, not the page bottom.
  const lastId = exchanges[exchanges.length - 1]?.id;
  useEffect(() => {
    if (pending !== null) endRef.current?.scrollIntoView?.({ block: "end", behavior: "smooth" });
    else if (lastId) document.getElementById(anchor(lastId))?.scrollIntoView?.({ block: "start", behavior: "smooth" });
  }, [lastId, pending]);

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

  // ↑/↓ move between results.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const rows = Array.from(document.querySelectorAll<HTMLElement>("[data-result]"));
    const i = rows.indexOf(document.activeElement as HTMLElement);
    if (i < 0 && !rows.length) return;
    e.preventDefault();
    const next = e.key === "ArrowDown" ? Math.min(rows.length - 1, i + 1) : Math.max(0, i - 1);
    rows[next]?.focus();
  };

  return (
    <div className="thread" onKeyDown={onKeyDown} aria-live="polite">
      {exchanges.map((x) => (
        <section key={x.id} id={anchor(x.id)} className="qa" aria-label={x.question?.text ?? undefined}>
          {x.question && <Question turn={x.question} />}
          {x.reply && <Reply turn={x.reply} isLast={x.reply === lastAi && !pending} {...props} />}
        </section>
      ))}
      {pending !== null && (
        <section className="qa">
          {pending && (
            <div className="qa-q">
              <h2 className="qa-q-text">{pending}</h2>
            </div>
          )}
          <Typing />
        </section>
      )}
      {error && (
        <div className="qa-answer empty" role="alert">
          <p className="qa-lead"><strong>{t(`error.${error.code}.title`)}</strong></p>
          <p className="qa-sub">{t(`error.${error.code}.body`)}</p>
        </div>
      )}
      <div ref={endRef} />
    </div>
  );
}
