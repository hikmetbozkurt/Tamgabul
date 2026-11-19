// Pure browser audio encoding/decoding + ephemeral microphone capture for P6.
// Audio stays strictly in RAM and is discarded immediately on stop or cancel.

export const TARGET_SAMPLE_RATE = 16_000;

export function downsampleTo16kPcm16(
  samples: Float32Array,
  inputSampleRate: number,
): Int16Array {
  if (samples.length === 0 || inputSampleRate <= 0) {
    return new Int16Array(0);
  }
  const ratio = inputSampleRate / TARGET_SAMPLE_RATE;
  const outLen = Math.max(1, Math.floor(samples.length / ratio));
  const out = new Int16Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const j = Math.floor(pos);
    const frac = pos - j;
    const a = samples[Math.min(j, samples.length - 1)];
    const b = samples[Math.min(j + 1, samples.length - 1)];
    const s = Math.max(-1, Math.min(1, a + (b - a) * frac));
    out[i] = Math.round(s * 0x7fff);
  }
  return out;
}

export function encodeWav16kMono(
  pcm: Int16Array,
  sampleRate = TARGET_SAMPLE_RATE,
): Uint8Array {
  const dataBytes = pcm.length * 2;
  const buf = new ArrayBuffer(44 + dataBytes);
  const view = new DataView(buf);
  const writeAscii = (offset: number, str: string) => {
    for (let i = 0; i < str.length; i++) {
      view.setUint8(offset + i, str.charCodeAt(i));
    }
  };
  writeAscii(0, "RIFF");
  view.setUint32(4, 36 + dataBytes, true);
  writeAscii(8, "WAVE");
  writeAscii(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeAscii(36, "data");
  view.setUint32(40, dataBytes, true);
  for (let i = 0; i < pcm.length; i++) {
    view.setInt16(44 + i * 2, pcm[i], true);
  }
  return new Uint8Array(buf);
}

export function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
}

export function base64ToBlobUrl(base64: string, mime = "audio/wav"): { url: string; revoke: boolean } {
  if (typeof URL.createObjectURL !== "function") {
    return { url: `data:${mime};base64,${base64}`, revoke: false };
  }
  const bin = atob(base64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return { url: URL.createObjectURL(new Blob([bytes], { type: mime })), revoke: true };
}

export function isMicDeniedError(err: unknown): boolean {
  if (!err || typeof err !== "object") return false;
  const msg = "message" in err ? String((err as { message?: unknown }).message ?? "") : "";
  const name = "name" in err ? String((err as { name?: unknown }).name ?? "") : "";
  const code = "code" in err ? String((err as { code?: unknown }).code ?? "") : "";
  return (
    code === "MIC_PERMISSION_DENIED"
    || msg === "MIC_PERMISSION_DENIED"
    || name === "NotAllowedError"
    || name === "PermissionDeniedError"
    || name === "SecurityError"
  );
}

export type MicSession = {
  stop: () => Promise<Uint8Array>;
  cancel: () => void;
};

export async function startMicCapture(): Promise<MicSession> {
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
    throw new Error("MIC_PERMISSION_DENIED");
  }
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const AudioCtx = window.AudioContext
    ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AudioCtx) {
    stream.getTracks().forEach((t) => t.stop());
    throw new Error("MIC_PERMISSION_DENIED");
  }

  const ctx = new AudioCtx();
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  let total = 0;
  let closed = false;

  processor.onaudioprocess = (ev) => {
    if (closed) return;
    const ch = ev.inputBuffer.getChannelData(0);
    chunks.push(new Float32Array(ch));
    total += ch.length;
  };
  source.connect(processor);
  processor.connect(ctx.destination);

  const release = () => {
    if (closed) return;
    closed = true;
    try { processor.disconnect(); } catch { /* ignore */ }
    try { source.disconnect(); } catch { /* ignore */ }
    stream.getTracks().forEach((t) => t.stop());
    ctx.close().catch(() => {});
  };

  return {
    cancel: () => {
      release();
      chunks.length = 0;
    },
    stop: async () => {
      const rate = ctx.sampleRate || 48_000;
      release();
      const merged = new Float32Array(total);
      let offset = 0;
      for (const c of chunks) {
        merged.set(c, offset);
        offset += c.length;
      }
      chunks.length = 0;
      return encodeWav16kMono(downsampleTo16kPcm16(merged, rate));
    },
  };
}
