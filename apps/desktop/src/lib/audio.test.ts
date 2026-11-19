import { describe, expect, it } from "vitest";
import {
  bytesToBase64,
  downsampleTo16kPcm16,
  encodeWav16kMono,
  isMicDeniedError,
} from "./audio";

describe("audio encoding and mic error classification", () => {
  it("downsamples to 16 kHz mono int16 PCM and builds a valid RIFF/WAVE header", () => {
    // 0.25 s at 48 kHz -> ~0.25 s at 16 kHz (4000 samples)
    const src = new Float32Array(12_000);
    for (let i = 0; i < src.length; i++) {
      src[i] = Math.sin((2 * Math.PI * 440 * i) / 48_000) * 0.5;
    }
    const pcm = downsampleTo16kPcm16(src, 48_000);
    expect(pcm.length).toBe(4000);

    const wav = encodeWav16kMono(pcm, 16_000);
    expect(wav.length).toBe(44 + 4000 * 2);
    expect(String.fromCharCode(...wav.slice(0, 4))).toBe("RIFF");
    expect(String.fromCharCode(...wav.slice(8, 12))).toBe("WAVE");
    const view = new DataView(wav.buffer);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint16(34, true)).toBe(16);

    const b64 = bytesToBase64(wav.slice(0, 12));
    expect(atob(b64).slice(0, 4)).toBe("RIFF");
  });

  it("recognizes browser microphone permission denial errors", () => {
    expect(isMicDeniedError(new DOMException("denied", "NotAllowedError"))).toBe(true);
    expect(isMicDeniedError(new Error("MIC_PERMISSION_DENIED"))).toBe(true);
    expect(isMicDeniedError({ code: "INTERNAL" })).toBe(false);
  });
});
