//! Smoke test run during "Add model" (docs/04 step 4): start a private worker,
//! embed a little text (and a generated image / tone when the encoders are
//! present), check the vectors, stop the worker. Uses no user data.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::client::{ChatMessage, Client};
use crate::eg2::{self, Task};
use crate::error::RuntimeError;
use crate::worker::{LlamaServerConfig, ProcessLauncher, Worker};

#[derive(Debug, Clone, PartialEq)]
pub struct SmokeReport {
    pub cold_load: Duration,
    pub dim: usize,
    /// `None` = not tested (no encoder).
    pub image: Option<bool>,
    pub audio: Option<bool>,
}

fn check(v: &[f32]) -> Result<(), RuntimeError> {
    if v.len() != eg2::NATIVE_DIM || !eg2::is_valid_vector(v) {
        return Err(RuntimeError::BadResponse(format!(
            "invalid embedding ({} values)",
            v.len()
        )));
    }
    Ok(())
}

pub fn eg2_smoke(cfg: LlamaServerConfig, cancel: &AtomicBool) -> Result<SmokeReport, RuntimeError> {
    let launcher = ProcessLauncher::llama_server(cfg);
    let t = Instant::now();
    let worker = launcher.launch_process(cancel)?;
    let cold_load = t.elapsed();
    let result = (|| {
        let c = Client::new(worker.endpoint().clone(), launcher.request_timeout());
        let props = c.props()?;
        let v = c.embed_texts(&[
            eg2::query_prompt(Task::SearchResult, "where is the meeting summary"),
            eg2::document_prompt(
                Some("notes.md"),
                "Meeting summary: the release moves to Friday.",
            ),
        ])?;
        v.iter().try_for_each(|x| check(x))?;
        let media = |bytes: &[u8]| -> Result<bool, RuntimeError> {
            let m = c.embed_media(&props.media_marker, &props.media_marker, &[bytes])?;
            check(&m)?;
            Ok(true)
        };
        let image = props.vision.then(|| media(&test_png(64, 64))).transpose()?;
        let audio = props.audio.then(|| media(&test_wav(1.0))).transpose()?;
        Ok(SmokeReport {
            cold_load,
            dim: v[0].len(),
            image,
            audio,
        })
    })();
    // Always stop the private worker, success or not.
    worker.terminate()?;
    result
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmSmokeReport {
    pub cold_load: Duration,
    /// One constrained answer, prompt + generation.
    pub answer: Duration,
    /// Image input works (`None` = no projector).
    pub vision: Option<bool>,
}

/// LLM smoke: start a private worker, ask one question whose answer is
/// fixed by a JSON schema, check the model picked the right enum value,
/// stop the worker. Proves the runtime supports the architecture, the chat
/// template works and grammar-constrained output works.
pub fn llm_smoke(
    cfg: LlamaServerConfig,
    cancel: &AtomicBool,
) -> Result<LlmSmokeReport, RuntimeError> {
    let has_mmproj = cfg.mmproj.is_some();
    let launcher = ProcessLauncher::llama_server(cfg);
    let t = Instant::now();
    let worker = launcher.launch_process(cancel)?;
    let cold_load = t.elapsed();
    let result = (|| {
        let c = Client::new(worker.endpoint().clone(), launcher.request_timeout());
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["capital"],
            "properties": { "capital": { "enum": ["Istanbul", "Ankara", "Izmir"] } }
        });
        let t = Instant::now();
        let v = c.chat_json(
            &[
                ChatMessage::system("Answer with JSON only."),
                ChatMessage::user("Türkiye'nin başkenti hangi şehirdir?"),
            ],
            &schema,
            32,
        )?;
        let answer = t.elapsed();
        if v["capital"] != "Ankara" {
            return Err(RuntimeError::BadResponse(
                "model gave a wrong answer to a fixed question".into(),
            ));
        }
        // With a projector: one image in, schema-shaped JSON out.
        let vision = if has_mmproj {
            let s = json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["what"],
                "properties": { "what": { "type": "string", "maxLength": 80 } }
            });
            Some(
                c.chat_json(
                    &[ChatMessage::user_with_png(
                        "What is in this image?",
                        test_png(64, 64),
                    )],
                    &s,
                    48,
                )
                .map(|v| v["what"].is_string())
                .unwrap_or(false),
            )
        } else {
            None
        };
        Ok(LlmSmokeReport {
            cold_load,
            answer,
            vision,
        })
    })();
    worker.terminate()?;
    result
}

// ---------------------------------------------------------------- fixtures

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A small RGB gradient PNG (uncompressed deflate blocks). Generated, so no
/// image file ships with the app.
pub fn test_png(w: u32, h: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            raw.extend([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 160]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, ch) in chunks.iter().enumerate() {
        z.push((i == chunks.len() - 1) as u8);
        let len = ch.len() as u16;
        z.extend(len.to_le_bytes());
        z.extend((!len).to_le_bytes());
        z.extend(*ch);
    }
    z.extend(adler32(&raw).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        png.extend(&body);
        png.extend(crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]); // 8-bit RGB
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    png
}

/// 16 kHz mono 16-bit WAV with a 440 Hz tone.
pub fn test_wav(seconds: f32) -> Vec<u8> {
    let rate = 16_000u32;
    let n = (rate as f32 * seconds) as u32;
    let mut pcm = Vec::with_capacity(n as usize * 2);
    for i in 0..n {
        let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
        pcm.extend(((s * 0.3 * i16::MAX as f32) as i16).to_le_bytes());
    }
    let mut w = b"RIFF".to_vec();
    w.extend((36 + pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes()); // PCM
    w.extend(1u16.to_le_bytes()); // mono
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend((pcm.len() as u32).to_le_bytes());
    w.extend(pcm);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_well_formed() {
        let p = test_png(64, 64);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(&p[p.len() - 8..p.len() - 4], b"IEND");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn wav_header() {
        let w = test_wav(0.5);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(w.len(), 44 + 8000 * 2);
    }
}
