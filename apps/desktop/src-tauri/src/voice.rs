//! On-demand local STT & TTS commands (P6, ADR 019).
//!
//! - Nothing runs at startup and no audio bytes or transcripts are ever
//!   written to disk or included in logs.
//! - Microphone capture / playback live in the WebView; Rust only coordinates
//!   the active supervised worker (`whisper-server` or `piper`) over loopback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use tamgabul_core::dto::{
    SynthesizeRequest, SynthesizeResponse, TranscribeRequest, TranscribeResponse,
};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry::{self, ModelRecord};
use tamgabul_runtime::client::Endpoint;
use tamgabul_runtime::stt::WhisperClient;
use tamgabul_runtime::tts::{self, TtsClient};
use tauri::{AppHandle, Manager, State};

use crate::AppState;

const MAX_WAV_BYTES: usize = 20 * 1024 * 1024;
const MAX_TTS_CHARS: usize = 4000;

#[derive(Default)]
pub struct VoiceState {
    tts_cancel: Mutex<Option<Arc<AtomicBool>>>,
    active_tts_endpoint: Mutex<Option<Endpoint>>,
}

fn lock_err() -> IpcError {
    IpcError::new(ErrorCode::Internal, "state lock poisoned")
}

fn decode_wav_base64(wav_b64: &str) -> Result<Vec<u8>, IpcError> {
    let trimmed = wav_b64.trim();
    if trimmed.is_empty() {
        return Err(IpcError::new(ErrorCode::Internal, "empty audio"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| IpcError::new(ErrorCode::Internal, "invalid audio encoding"))?;
    if bytes.len() > MAX_WAV_BYTES {
        return Err(IpcError::new(
            ErrorCode::Internal,
            "audio recording too large",
        ));
    }
    if tts::parse_wav_sample_rate(&bytes).is_none() {
        return Err(IpcError::new(ErrorCode::Internal, "invalid WAV audio"));
    }
    Ok(bytes)
}

fn clean_tts_text(raw: &str) -> Result<String, IpcError> {
    let s = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return Err(IpcError::new(ErrorCode::Internal, "nothing to read"));
    }
    let capped = match s.char_indices().nth(MAX_TTS_CHARS) {
        Some((i, _)) => s[..i].to_string(),
        None => s,
    };
    Ok(capped)
}

fn resolve_active_model(state: &AppState, cat: ModelCategory) -> Result<ModelRecord, IpcError> {
    let rec = {
        let guard = state.db.lock().map_err(|_| lock_err())?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
        registry::active(conn)?
            .into_iter()
            .find(|m| m.category == cat)
            .filter(|m| m.status.from_persisted_snapshot() == ModelStatus::Available)
            .ok_or_else(|| IpcError::new(ErrorCode::ModelMissing, "voice model not installed"))?
    };
    if state.models.snapshot(&rec.model_id).is_none() {
        crate::models::attach(&state.models, &rec);
    }
    Ok(rec)
}

pub fn cancel_active(state: &AppState) {
    if let Ok(mut c) = state.voice.tts_cancel.lock() {
        if let Some(flag) = c.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
    let ep = state
        .voice
        .active_tts_endpoint
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    if let Some(ep) = ep {
        let _ = TtsClient::new(ep, Duration::from_secs(2)).cancel();
    }
}

#[tauri::command]
pub async fn transcribe_audio(
    app: AppHandle,
    req: TranscribeRequest,
) -> Result<TranscribeResponse, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let wav = decode_wav_base64(&req.wav_base64)?;
        let state = app.state::<AppState>();
        let rec = resolve_active_model(&state, ModelCategory::Stt)?;
        let lease = state
            .models
            .acquire(&rec.model_id)
            .map_err(crate::models::ipc)?;
        let client = WhisperClient::new(lease.endpoint().clone(), Duration::from_secs(60));
        let text = client
            .transcribe_wav(&wav, req.language.as_deref())
            .map_err(crate::models::ipc)?;
        Ok(TranscribeResponse { text })
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "transcription task failed"))?
}

#[tauri::command]
pub async fn synthesize_speech(
    app: AppHandle,
    req: SynthesizeRequest,
) -> Result<SynthesizeResponse, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let text = clean_tts_text(&req.text)?;
        let state = app.state::<AppState>();
        cancel_active(&state);
        let cancel = Arc::new(AtomicBool::new(false));
        *state.voice.tts_cancel.lock().map_err(|_| lock_err())? = Some(cancel.clone());

        let rec = resolve_active_model(&state, ModelCategory::Tts)?;
        let lease = state
            .models
            .acquire(&rec.model_id)
            .map_err(crate::models::ipc)?;
        if cancel.load(Ordering::SeqCst) {
            return Err(IpcError::new(ErrorCode::UserCancelled, "cancelled"));
        }

        let ep = lease.endpoint().clone();
        *state
            .voice
            .active_tts_endpoint
            .lock()
            .map_err(|_| lock_err())? = Some(ep.clone());

        let res = TtsClient::new(ep, Duration::from_secs(60)).synthesize(&text);

        if let Ok(mut g) = state.voice.active_tts_endpoint.lock() {
            *g = None;
        }
        if let Ok(mut g) = state.voice.tts_cancel.lock() {
            if g.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                *g = None;
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(IpcError::new(ErrorCode::UserCancelled, "cancelled"));
        }
        let wav = res.map_err(crate::models::ipc)?;
        let sample_rate = tts::parse_wav_sample_rate(&wav).unwrap_or(22_050);
        let wav_base64 = base64::engine::general_purpose::STANDARD.encode(&wav);
        Ok(SynthesizeResponse {
            wav_base64,
            sample_rate,
        })
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "speech task failed"))?
}

#[tauri::command]
pub fn stop_speech(state: State<'_, AppState>) -> Result<(), IpcError> {
    cancel_active(&state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_wav_base64_and_rejects_bad_headers() {
        let wav = tamgabul_runtime::stt::silent_wav_16khz(100);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav);
        assert_eq!(decode_wav_base64(&b64).unwrap(), wav);

        assert!(decode_wav_base64("").is_err());
        let bad_b64 = base64::engine::general_purpose::STANDARD.encode(b"not a wav file");
        assert!(decode_wav_base64(&bad_b64).is_err());
    }

    #[test]
    fn cleans_and_bounds_tts_text() {
        assert!(clean_tts_text("   ").is_err());
        assert_eq!(clean_tts_text("  hello \n  world ").unwrap(), "hello world");
        let long = "a".repeat(5000);
        assert_eq!(
            clean_tts_text(&long).unwrap().chars().count(),
            MAX_TTS_CHARS
        );
    }
}
