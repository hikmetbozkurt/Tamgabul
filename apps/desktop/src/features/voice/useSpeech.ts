import { useCallback, useEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import { toIpcError } from "../../ipc/bridge";
import { base64ToBlobUrl } from "../../lib/audio";
import { useData } from "../../state/data";
import { useToast } from "../../state/toast";

export type SpeechController = {
  speakingId: string | null;
  loadingId: string | null;
  speak: (id: string, text: string) => void;
  stop: () => void;
};

/** Stoppable local TTS playback for answers and summaries (P6). */
export function useSpeech(): SpeechController {
  const { api } = useData();
  const t = useT();
  const toast = useToast();
  const [speakingId, setSpeakingId] = useState<string | null>(null);
  const [loadingId, setLoadingId] = useState<string | null>(null);

  const genRef = useRef(0);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const revokeUrlRef = useRef<string | null>(null);

  const cleanupAudio = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.onended = null;
      audioRef.current.onerror = null;
      try { audioRef.current.pause(); } catch { /* ignore */ }
      audioRef.current = null;
    }
    if (revokeUrlRef.current) {
      URL.revokeObjectURL(revokeUrlRef.current);
      revokeUrlRef.current = null;
    }
  }, []);

  const stop = useCallback(() => {
    genRef.current += 1;
    cleanupAudio();
    setLoadingId(null);
    setSpeakingId(null);
    api.stopSpeech().catch(() => {});
  }, [api, cleanupAudio]);

  useEffect(() => () => {
    genRef.current += 1;
    cleanupAudio();
    api.stopSpeech().catch(() => {});
  }, [api, cleanupAudio]);

  const speak = useCallback((id: string, text: string) => {
    if (!text.trim()) return;
    if (speakingId === id || loadingId === id) {
      stop();
      return;
    }
    cleanupAudio();
    const myGen = ++genRef.current;
    setSpeakingId(null);
    setLoadingId(id);

    api.synthesizeSpeech({ text })
      .then((res) => {
        if (genRef.current !== myGen) return;
        const { url, revoke } = base64ToBlobUrl(res.wav_base64, "audio/wav");
        if (revoke) revokeUrlRef.current = url;
        const audio = new Audio(url);
        audioRef.current = audio;
        const finish = () => {
          if (genRef.current !== myGen) return;
          cleanupAudio();
          setSpeakingId(null);
        };
        audio.onended = finish;
        audio.onerror = finish;
        setLoadingId(null);
        setSpeakingId(id);
        const played = audio.play();
        if (played && typeof played.catch === "function") {
          played.catch(() => { /* playback blocked/unsupported in test DOM; keep stoppable state */ });
        }
      })
      .catch((err) => {
        if (genRef.current !== myGen) return;
        setLoadingId(null);
        setSpeakingId(null);
        const ipc = toIpcError(err);
        if (ipc.code !== "USER_CANCELLED") {
          toast(t(`error.${ipc.code}.title`), true);
        }
      });
  }, [api, cleanupAudio, loadingId, speakingId, stop, t, toast]);

  return { speakingId, loadingId, speak, stop };
}
