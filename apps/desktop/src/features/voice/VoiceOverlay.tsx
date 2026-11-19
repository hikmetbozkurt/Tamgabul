import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../ui/icons";
import { useT } from "../../i18n";
import { toIpcError } from "../../ipc/bridge";
import { bytesToBase64, isMicDeniedError, type MicSession, startMicCapture } from "../../lib/audio";
import { useData } from "../../state/data";
import { useSettings } from "../../state/settings";

type Phase = "listening" | "transcribing" | "review" | "error";

/**
 * Push-to-talk / toggle voice capture view (W3, P6).
 * Captures 16 kHz mono WAV strictly in memory, transcribes locally via
 * `whisper-server`, and always allows transcript review/editing before search.
 */
export function VoiceView({ onCancel, onSubmit }: { onCancel: () => void; onSubmit: (text: string) => void }) {
  const t = useT();
  const { api } = useData();
  const { settings } = useSettings();
  const [phase, setPhase] = useState<Phase>("listening");
  const [secs, setSecs] = useState(0);
  const [text, setText] = useState("");
  const [errorCode, setErrorCode] = useState<string>("MIC_PERMISSION_DENIED");

  const inputRef = useRef<HTMLInputElement>(null);
  const micRef = useRef<MicSession | null>(null);
  const genRef = useRef(0);

  const beginRecording = useCallback(() => {
    const myGen = ++genRef.current;
    micRef.current?.cancel();
    micRef.current = null;
    setSecs(0);
    setText("");
    setPhase("listening");

    startMicCapture()
      .then((session) => {
        if (genRef.current !== myGen) {
          session.cancel();
          return;
        }
        micRef.current = session;
      })
      .catch((err) => {
        if (genRef.current !== myGen) return;
        const code = isMicDeniedError(err) ? "MIC_PERMISSION_DENIED" : toIpcError(err).code;
        setErrorCode(code);
        setPhase("error");
      });
  }, []);

  useEffect(() => {
    beginRecording();
    return () => {
      genRef.current += 1;
      micRef.current?.cancel();
      micRef.current = null;
    };
  }, [beginRecording]);

  useEffect(() => {
    if (phase !== "listening") return;
    const tick = setInterval(() => setSecs((s) => s + 1), 1000);
    return () => clearInterval(tick);
  }, [phase]);

  const stop = useCallback(async () => {
    const myGen = ++genRef.current;
    const session = micRef.current;
    micRef.current = null;
    setPhase("transcribing");
    try {
      const wav = session ? await session.stop() : new Uint8Array(0);
      if (genRef.current !== myGen) return;
      if (wav.length <= 44) {
        setText("");
        setPhase("review");
        setTimeout(() => inputRef.current?.focus(), 0);
        return;
      }
      const res = await api.transcribeAudio({ wav_base64: bytesToBase64(wav), language: null });
      if (genRef.current !== myGen) return;
      setText(res.text);
      setPhase("review");
      setTimeout(() => inputRef.current?.focus(), 0);
    } catch (err) {
      if (genRef.current !== myGen) return;
      const code = toIpcError(err).code;
      setErrorCode(code);
      setPhase("error");
    }
  }, [api]);

  const cancel = useCallback(() => {
    genRef.current += 1;
    micRef.current?.cancel();
    micRef.current = null;
    onCancel();
  }, [onCancel]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopPropagation(); cancel(); }
      if (e.key === "Enter" && phase === "listening") stop();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [cancel, phase, stop]);

  const bars = [8, 16, 26, 14, 22, 10, 18, 6];
  const mmss = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
  const ringLabel = phase === "listening" ? t("voice.stop") : t("voice.again");

  return (
    <div className="voice">
      <div className="voice-body">
        <button
          type="button"
          className={`voice-ring${phase === "listening" ? " live" : ""}`}
          disabled={phase === "transcribing"}
          aria-label={ringLabel}
          onClick={phase === "listening" ? () => { void stop(); } : beginRecording}
        >
          <span className="voice-core">
            {phase === "transcribing" ? (
              <span className="spinner" aria-hidden="true" />
            ) : (
              <Icon name={phase === "listening" ? "stop" : "mic"} size={28} stroke={1.8} />
            )}
          </span>
        </button>

        {phase === "listening" && (
          <div className="voice-bars" aria-hidden="true">
            {bars.map((h, i) => <span key={i} style={{ height: h, animationDelay: `${i * 90}ms` }} />)}
          </div>
        )}

        <div style={{ textAlign: "center" }}>
          <div role="status" className="voice-state">
            {phase === "listening" && t("voice.listening", { time: mmss })}
            {phase === "transcribing" && t("voice.transcribing")}
            {phase === "review" && t("voice.review")}
            {phase === "error" && t(`error.${errorCode}.title`)}
          </div>
          <div className="small muted" style={{ marginTop: 4 }}>
            {phase === "listening"
              ? settings.mic_mode === "push_to_talk" ? t("voice.releaseHint") : t("voice.clickHint")
              : phase === "error"
                ? t(`error.${errorCode}.body`)
                : t("voice.notStored")}
          </div>
        </div>

        {phase === "review" && (
          <div className="voice-transcript">
            <label htmlFor="transcript" className="section-label" style={{ fontSize: 12, textTransform: "none", letterSpacing: 0 }}>
              {t("voice.heard")}
            </label>
            <input
              id="transcript"
              ref={inputRef}
              value={text}
              placeholder={t("voice.empty")}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && text.trim() && onSubmit(text)}
            />
          </div>
        )}
      </div>

      <div className="voice-foot">
        <button type="button" className="btn btn-sm" onClick={cancel}>
          {t("common.cancel")} <span className="muted">Esc</span>
        </button>
        {phase === "listening" && (
          <button type="button" className="btn btn-sm btn-primary" onClick={() => { void stop(); }}>
            {t("voice.stop")}
          </button>
        )}
        {phase === "error" && (
          <button type="button" className="btn btn-sm btn-primary" onClick={beginRecording}>
            {t("common.retry")}
          </button>
        )}
        {phase === "review" && (
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
