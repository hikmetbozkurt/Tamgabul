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

/// One chat message for [`Client::chat_json`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
    /// PNG/JPEG bytes sent with the text (needs a worker with a projector).
    pub image: Option<Vec<u8>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system",
            content: content.into(),
            image: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
            image: None,
        }
    }
    pub fn user_with_png(content: impl Into<String>, png: Vec<u8>) -> Self {
        Self {
            role: "user",
            content: content.into(),
            image: Some(png),
        }
    }

    fn to_json(&self) -> Value {
        match &self.image {
            None => json!({ "role": self.role, "content": self.content }),
            Some(img) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(img);
                json!({ "role": self.role, "content": [
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{b64}") } },
                    { "type": "text", "text": self.content }
                ] })
            }
        }
    }
}

impl Client {
    /// Chat completion whose output is constrained by `schema` (llama-server
    /// turns the JSON Schema into a grammar, so the model cannot produce
    /// anything else). Greedy decoding, thinking off. Returns the parsed
    /// object; the caller still validates every field.
    pub fn chat_json(
        &self,
        messages: &[ChatMessage],
        schema: &Value,
        max_tokens: u32,
    ) -> Result<Value, RuntimeError> {
        let msgs: Vec<Value> = messages.iter().map(ChatMessage::to_json).collect();
        let v: Value = self
            .agent
            .post(&self.url("/v1/chat/completions"))
            .set("Authorization", &self.auth())
            .send_json(json!({
                "messages": msgs,
                "temperature": 0.0,
                "top_k": 1,
                "max_tokens": max_tokens,
                "cache_prompt": true,
                "chat_template_kwargs": { "enable_thinking": false },
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "plan", "strict": true, "schema": schema }
                }
            }))
            .map_err(Self::map_err)?
            .into_json()
            .map_err(|e| RuntimeError::BadResponse(e.to_string()))?;
        parse_chat_json(&v)
    }
}

/// `choices[0].message.content` parsed as a JSON object.
pub fn parse_chat_json(v: &Value) -> Result<Value, RuntimeError> {
    let choice = &v["choices"][0];
    if choice["finish_reason"].as_str() == Some("length") {
        return Err(RuntimeError::BadResponse("output cut off".into()));
    }
    let content = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| RuntimeError::BadResponse("missing message content".into()))?;
    let out: Value = serde_json::from_str(content.trim())
        .map_err(|_| RuntimeError::BadResponse("output is not JSON".into()))?;
    if !out.is_object() {
        return Err(RuntimeError::BadResponse("output is not an object".into()));
    }
    Ok(out)
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

    /// One-shot HTTP stub: returns the raw request it received.
    fn stub(reply: &'static str) -> (Endpoint, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            let mut s = s;
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
            head + &String::from_utf8(body).unwrap()
        });
        (
            Endpoint {
                base_url: format!("http://127.0.0.1:{port}"),
                token: "tok".into(),
            },
            h,
        )
    }

    #[test]
    fn chat_json_sends_schema_greedy_and_token() {
        let (ep, h) = stub(
            r#"{"choices":[{"finish_reason":"stop","message":{"content":"{\"capital\":\"Ankara\"}"}}]}"#,
        );
        let c = Client::new(ep, Duration::from_secs(5));
        let schema = json!({"type":"object","properties":{"capital":{"enum":["Ankara"]}}});
        let v = c
            .chat_json(
                &[ChatMessage::system("s"), ChatMessage::user("u")],
                &schema,
                16,
            )
            .unwrap();
        assert_eq!(v["capital"], "Ankara");
        let req = h.join().unwrap();
        assert!(req.starts_with("POST /v1/chat/completions"));
        assert!(req.contains("Bearer tok"));
        let body: Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["max_tokens"], 16);
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn image_messages_use_a_data_url_part() {
        let m = ChatMessage::user_with_png("describe", vec![1, 2, 3]).to_json();
        assert_eq!(
            m["content"][0]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
        assert_eq!(m["content"][1]["text"], "describe");
        assert_eq!(ChatMessage::user("hi").to_json()["content"], "hi");
    }

    #[test]
    fn chat_output_must_be_a_complete_object() {
        let ok = json!({"choices":[{"finish_reason":"stop","message":{"content":" {\"a\":1} "}}]});
        assert_eq!(parse_chat_json(&ok).unwrap()["a"], 1);
        let cut = json!({"choices":[{"finish_reason":"length","message":{"content":"{\"a\":"}}]});
        assert!(parse_chat_json(&cut).is_err());
        let prose = json!({"choices":[{"message":{"content":"Sure! here"}}]});
        assert!(parse_chat_json(&prose).is_err());
        let arr = json!({"choices":[{"message":{"content":"[1]"}}]});
        assert!(parse_chat_json(&arr).is_err());
        assert!(parse_chat_json(&json!({})).is_err());
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
