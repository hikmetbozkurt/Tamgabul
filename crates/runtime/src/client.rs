//! HTTP client for a worker bound to 127.0.0.1 (llama-server API).
//!
//! Every request carries the per-process bearer token. The agent ignores
//! proxy environment variables, so nothing leaves the machine.

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::error::RuntimeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// e.g. `http://127.0.0.1:49152`
    pub base_url: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Props {
    pub vision: bool,
    pub audio: bool,
    /// Placeholder the server expects for each media item in a prompt.
    pub media_marker: String,
    pub build_info: Option<String>,
}

pub const DEFAULT_MEDIA_MARKER: &str = "<__media__>";

pub struct Client {
    ep: Endpoint,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(ep: Endpoint, request_timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(request_timeout)
            .try_proxy_from_env(false)
            .build();
        Self { ep, agent }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.ep
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.ep.base_url)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.ep.token)
    }

    fn map_err(e: ureq::Error) -> RuntimeError {
        match e {
            ureq::Error::Status(code, r) => {
                let body = r.into_string().unwrap_or_default();
                RuntimeError::Request(format!("HTTP {code}: {}", truncate(&body, 300)))
            }
            ureq::Error::Transport(t) => {
                let s = t.to_string();
                if s.contains("timed out") || s.contains("Timeout") {
                    RuntimeError::Timeout
                } else {
                    RuntimeError::Request(s)
                }
            }
        }
    }

    /// `Ok(true)` when the model is loaded, `Ok(false)` while loading (503).
    pub fn health(&self) -> Result<bool, RuntimeError> {
        match self
            .agent
            .get(&self.url("/health"))
            .set("Authorization", &self.auth())
            .call()
        {
            Ok(r) => Ok(r.status() == 200),
            Err(ureq::Error::Status(503, _)) => Ok(false),
            Err(e) => Err(Self::map_err(e)),
        }
    }

    pub fn props(&self) -> Result<Props, RuntimeError> {
        let v: Value = self
            .agent
            .get(&self.url("/props"))
            .set("Authorization", &self.auth())
            .call()
            .map_err(Self::map_err)?
            .into_json()
            .map_err(|e| RuntimeError::BadResponse(e.to_string()))?;
        Ok(parse_props(&v))
    }

    /// Text embeddings via the OpenAI-compatible endpoint, in input order.
    pub fn embed_texts(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RuntimeError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let v: Value = self
            .agent
            .post(&self.url("/v1/embeddings"))
            .set("Authorization", &self.auth())
            .send_json(json!({ "input": inputs, "encoding_format": "float" }))
            .map_err(Self::map_err)?
            .into_json()
            .map_err(|e| RuntimeError::BadResponse(e.to_string()))?;
        parse_oai_embeddings(&v, inputs.len())
    }

    /// One embedding for a prompt containing media. `prompt` must contain
    /// exactly one `marker` per item in `media` (raw file bytes: PNG/JPEG,
    /// WAV/MP3/FLAC). Uses the non-OAI `/embedding` endpoint, which the
    /// llama-server README lists as multimodal-capable.
    pub fn embed_media(
        &self,
        prompt: &str,
        marker: &str,
        media: &[&[u8]],
    ) -> Result<Vec<f32>, RuntimeError> {
        if prompt.matches(marker).count() != media.len() {
            return Err(RuntimeError::Request("media marker count mismatch".into()));
        }
        let b64 = base64::engine::general_purpose::STANDARD;
        let data: Vec<String> = media.iter().map(|m| b64.encode(m)).collect();
        let v: Value = self
            .agent
            .post(&self.url("/embedding"))
            .set("Authorization", &self.auth())
            .send_json(json!({
                "content": { "prompt_string": prompt, "multimodal_data": data }
            }))
            .map_err(Self::map_err)?
            .into_json()
            .map_err(|e| RuntimeError::BadResponse(e.to_string()))?;
        parse_native_embedding(&v)
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

pub fn parse_props(v: &Value) -> Props {
    let m = &v["modalities"];
    Props {
        vision: m["vision"].as_bool().unwrap_or(false),
        audio: m["audio"].as_bool().unwrap_or(false),
        media_marker: v["media_marker"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_MEDIA_MARKER)
            .to_owned(),
        build_info: v["build_info"].as_str().map(str::to_owned),
    }
}

fn floats(v: &Value) -> Result<Vec<f32>, RuntimeError> {
    v.as_array()
        .ok_or_else(|| RuntimeError::BadResponse("embedding is not an array".into()))?
        .iter()
        .map(|x| {
            x.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| RuntimeError::BadResponse("non-numeric embedding value".into()))
        })
        .collect()
}

pub fn parse_oai_embeddings(v: &Value, expected: usize) -> Result<Vec<Vec<f32>>, RuntimeError> {
    let data = v["data"]
        .as_array()
        .ok_or_else(|| RuntimeError::BadResponse("missing data[]".into()))?;
    if data.len() != expected {
        return Err(RuntimeError::BadResponse(format!(
            "expected {expected} embeddings, got {}",
            data.len()
        )));
    }
    let mut out: Vec<Option<Vec<f32>>> = vec![None; expected];
    for (pos, item) in data.iter().enumerate() {
        let idx = item["index"].as_u64().map(|i| i as usize).unwrap_or(pos);
        let slot = out
            .get_mut(idx)
            .ok_or_else(|| RuntimeError::BadResponse("index out of range".into()))?;
        *slot = Some(floats(&item["embedding"])?);
    }
    out.into_iter()
        .map(|o| o.ok_or_else(|| RuntimeError::BadResponse("missing index".into())))
        .collect()
}

/// `/embedding` answers `[{index, embedding: [[..]]}]` (pooled: one row) or,
/// in older builds, `{embedding: [..]}`.
pub fn parse_native_embedding(v: &Value) -> Result<Vec<f32>, RuntimeError> {
    let item = match v {
        Value::Array(a) if a.len() == 1 => &a[0],
        Value::Array(a) => {
            return Err(RuntimeError::BadResponse(format!(
                "expected 1 result, got {}",
                a.len()
            )))
        }
        other => other,
    };
    let e = &item["embedding"];
    match e.as_array().and_then(|a| a.first()) {
        Some(Value::Array(_)) => {
            let rows = e.as_array().map(Vec::len).unwrap_or(0);
            if rows != 1 {
                return Err(RuntimeError::BadResponse(format!(
                    "{rows} rows: server is not pooling (per-token output)"
                )));
            }
            floats(&e[0])
        }
        Some(_) => floats(e),
        None => Err(RuntimeError::BadResponse("empty embedding".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oai_embeddings_are_reordered_by_index() {
        let v = json!({"data":[
            {"index":1,"embedding":[0.0,1.0]},
            {"index":0,"embedding":[1.0,0.0]}
        ]});
        let out = parse_oai_embeddings(&v, 2).unwrap();
        assert_eq!(out[0], vec![1.0, 0.0]);
        assert_eq!(out[1], vec![0.0, 1.0]);
        assert!(parse_oai_embeddings(&v, 3).is_err());
    }

    #[test]
    fn native_embedding_shapes() {
        assert_eq!(
            parse_native_embedding(&json!([{"index":0,"embedding":[[1.0,2.0]]}])).unwrap(),
            vec![1.0, 2.0]
        );
        assert_eq!(
            parse_native_embedding(&json!({"embedding":[3.0]})).unwrap(),
            vec![3.0]
        );
        assert!(matches!(
            parse_native_embedding(&json!([{"embedding":[[1.0],[2.0]]}])),
            Err(RuntimeError::BadResponse(_))
        ));
    }

    #[test]
    fn props_defaults() {
        let p = parse_props(&json!({"modalities":{"vision":true,"audio":true}}));
        assert!(p.vision && p.audio);
        assert_eq!(p.media_marker, DEFAULT_MEDIA_MARKER);
        let p = parse_props(&json!({"media_marker":"<m>"}));
        assert!(!p.vision);
        assert_eq!(p.media_marker, "<m>");
    }
}
