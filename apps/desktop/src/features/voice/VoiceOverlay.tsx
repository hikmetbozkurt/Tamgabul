import { useEffect, useRef, useState } from "react";
import { Icon } from "../../ui/icons";
import { useT } from "../../i18n";
import { useData } from "../../state/data";
import { useSettings } from "../../state/settings";

/**
 * Push-to-talk capture view (W3). The real STT runtime arrives in P6; until
 * then this never touches the microphone — the demo shows the flow only.
 * Visible recording indicator + timer + cancel are functional contracts.
 */
export function VoiceView({ onCancel, onSubmit }: { onCancel: () => void; onSubmit: (text: string) => void }) {
  const t = useT();
  const { api } = useData();
  const { settings } = useSettings();
  const [phase, setPhase] = useState<"listening" | "review">("listening");
  const [secs, setSecs] = useState(0);
  const [text, setText] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (phase !== "listening") return;
    const tick = setInterval(() => setSecs((s) => s + 1), 1000);
    return () => clearInterval(tick);
  }, [phase]);

  const stop = () => {
    setPhase("review");
    setText(api.mode === "mock" ? t("voice.demoTranscript") : "");
    setTimeout(() => inputRef.current?.focus(), 0);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopPropagation(); onCancel(); }
      if (e.key === "Enter" && phase === "listening") stop();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  });

  const bars = [8, 16, 26, 14, 22, 10, 18, 6];
  const mmss = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;

  return (
    <div className="voice">
      <div className="voice-body">
        <button
          type="button"
          className={`voice-ring${phase === "listening" ? " live" : ""}`}
          aria-label={phase === "listening" ? t("voice.stop") : t("voice.again")}
          onClick={phase === "listening" ? stop : () => { setSecs(0); setPhase("listening"); }}
        >
          <span className="voice-core"><Icon name={phase === "listening" ? "stop" : "mic"} size={28} stroke={1.8} /></span>
        </button>
        {phase === "listening" && (
          <div className="voice-bars" aria-hidden="true">
            {bars.map((h, i) => <span key={i} style={{ height: h, animationDelay: `${i * 90}ms` }} />)}
          </div>
        )}
        <div style={{ textAlign: "center" }}>
          <div role="status" className="voice-state">
            {phase === "listening" ? t("voice.listening", { time: mmss }) : t("voice.review")}
          </div>
          <div className="small muted" style={{ marginTop: 4 }}>
            {phase === "listening"
              ? settings.mic_mode === "push_to_talk" ? t("voice.releaseHint") : t("voice.clickHint")
              : t("voice.notStored")}
          </div>
          {api.mode === "mock" && <div className="small" style={{ marginTop: 6, color: "var(--warn-text)" }}>{t("voice.mockNote")}</div>}
        </div>
        {phase === "review" && (
          <div className="voice-transcript">
            <label htmlFor="transcript" className="section-label" style={{ fontSize: 12, textTransform: "none", letterSpacing: 0 }}>{t("voice.heard")}</label>
            <input
              id="transcript"
              ref={inputRef}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && text.trim() && onSubmit(text)}
            />
          </div>
        )}
      </div>
      <div className="voice-foot">
        <button type="button" className="btn btn-sm" onClick={onCancel}>{t("common.cancel")} <span className="muted">Esc</span></button>
        {phase === "listening" ? (
          <button type="button" className="btn btn-sm btn-primary" onClick={stop}>{t("voice.stop")}</button>
        ) : (
          <button type="button" className="btn btn-sm btn-primary" disabled={!text.trim()} onClick={() => onSubmit(text)}>
            {t("voice.search")} <span style={{ opacity: 0.6 }}>Enter</span>
          </button>
        )}
      </div>
    </div>
  );
}

export function VoiceOverlay({ onClose, onSubmit }: { onClose: () => void; onSubmit: (text: string) => void }) {
  const t = useT();
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog sm" role="dialog" aria-modal="true" aria-label={t("voice.title")} style={{ padding: 0, gap: 0 }}>
        <VoiceView onCancel={onClose} onSubmit={onSubmit} />
      </div>
    </div>
  );
}
