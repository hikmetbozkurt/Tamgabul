//! Small instruction LLM profile (docs/04 "LLM", docs/08).
//!
//! The LLM serves search only: it turns a request into a search plan,
//! picks the matching files among retrieved candidates and (with its
//! vision projector) describes images once at scan time. Output is always
//! grammar-constrained JSON; it never decides what may be opened. Any GGUF
//! instruction model with a chat template is accepted; the tested one is
//! Gemma 4 E2B-it from ggml-org.

use std::path::{Path, PathBuf};

use crate::eg2::file_type_name;
use crate::error::RuntimeError;
use crate::gguf::{read_header, GgufHeader};

/// LFS SHA-256 of `ggml-org/gemma-4-E2B-it-GGUF` files (Hugging Face API).
/// Other files work too; they are reported as unpinned.
pub const PINNED_REPO: &str = "ggml-org/gemma-4-E2B-it-GGUF";
pub const PINNED_SHA256: &[(&str, &str)] = &[
    (
        "gemma-4-E2B-it-Q4_0.gguf",
        "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
    ),
    (
        "gemma-4-E2B-it-Q8_0.gguf",
        "996d08777aadc6bfd3c7375ef70ba25a0f55240075860754fdb18d6d860aa63a",
    ),
    (
        "gemma-4-E2B-it-BF16.gguf",
        "246c5fb19e64b941b531e86639f070bc67b00f20e9ec8556a6c02653321bc65d",
    ),
    (
        "mmproj-gemma-4-E2B-it-Q8_0.gguf",
        "9406f99c16d68cda4f1f0552192dcc99021ea1fc6d2fd50b1dc3ccf30d04b292",
    ),
    (
        "mmproj-gemma-4-E2B-it-BF16.gguf",
        "711e1e8f43fa0664adbac493129be1e6c25b81af4b4cdea97c7d798b25c0a3a4",
    ),
];

pub fn is_pinned(sha256: &str) -> bool {
    PINNED_SHA256
        .iter()
        .any(|(_, h)| h.eq_ignore_ascii_case(sha256))
}

/// Architectures that produce vectors, not text.
const ENCODER_ARCHS: &[&str] = &[
    "bert",
    "nomic-bert",
    "nomic-bert-moe",
    "jina-bert-v2",
    "jina-bert-v3",
    "modern-bert",
    "neo-bert",
    "t5encoder",
    "gemma-embedding",
    "gemma-embedding2",
    "clip",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProbe {
    pub architecture: String,
    pub name: Option<String>,
    /// GGUF `general.file_type`, e.g. "Q4_0".
    pub precision: String,
    pub size_label: Option<String>,
    pub context_length: Option<u64>,
    pub license: Option<String>,
}

fn kind_error(h: &GgufHeader) -> Option<String> {
    let arch = h.architecture().unwrap_or("");
    if h.str("general.type").is_some_and(|t| t != "model") {
        return Some(format!(
            "general.type is '{}', expected a model",
            h.str("general.type").unwrap_or("")
        ));
    }
    if arch.is_empty() {
        return Some("architecture missing".into());
    }
    if ENCODER_ARCHS.contains(&arch) || h.get(&format!("{arch}.pooling_type")).is_some() {
        return Some(format!("'{arch}' is an embedding model, not a chat model"));
    }
    if h.str("tokenizer.chat_template")
        .is_none_or(|t| t.trim().is_empty())
    {
        return Some("no chat template: not an instruction model".into());
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmFiles {
    pub model: PathBuf,
    /// Vision projector of the same model (image descriptions). Optional.
    pub mmproj: Option<PathBuf>,
}

fn gguf_files(dir: &Path) -> Result<Vec<(u64, PathBuf)>, RuntimeError> {
    let rd = std::fs::read_dir(dir).map_err(|e| RuntimeError::Io(format!("read folder: {e}")))?;
    let mut files: Vec<(u64, PathBuf)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
        })
        .map(|p| (p.metadata().map(|m| m.len()).unwrap_or(0), p))
        .collect();
    // Largest first: with several quantizations, take the main weights.
    files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(files)
}

/// A projector with a vision encoder made for the model called `name`.
fn is_projector_for(h: &GgufHeader, name: Option<&str>) -> bool {
    h.str("general.type") == Some("mmproj")
        && h.bool("clip.has_vision_encoder") == Some(true)
        && name.is_some_and(|n| h.str("general.name") == Some(n))
}

/// The instruction model inside a file path or folder, chosen by GGUF
/// headers (never by file name), plus its vision projector when one sits
/// next to it with the same `general.name`. Draft/MTP heads are skipped
/// because they carry no chat template.
pub fn locate(path: &Path) -> Result<LlmFiles, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::ModelMissing);
    }
    let files = if path.is_file() {
        // The chosen file first, then its neighbours (for the projector).
        let mut files = vec![(0, path.to_path_buf())];
        if let Some(d) = path.parent() {
            files.extend(gguf_files(d)?.into_iter().filter(|(_, p)| p != path));
        }
        files
    } else {
        gguf_files(path)?
    };
    let mut model: Option<(PathBuf, Option<String>)> = None;
    let mut projectors: Vec<(PathBuf, GgufHeader)> = Vec::new();
    for (_, p) in files {
        let Ok(h) = read_header(&p) else { continue };
        if h.str("general.type") == Some("mmproj") {
            projectors.push((p, h));
        } else if model.is_none() && kind_error(&h).is_none() {
            model = Some((p, h.str("general.name").map(str::to_owned)));
        } else if model.is_none() && path.is_file() {
            // An explicitly chosen file that is not a chat model.
            return Err(RuntimeError::Incompatible(
                kind_error(&h).unwrap_or_else(|| "not a chat model".into()),
            ));
        }
    }
    let Some((model, name)) = model else {
        return Err(RuntimeError::Incompatible(
            "no GGUF instruction model (with chat template) found".into(),
        ));
    };
    let mmproj = projectors
        .into_iter()
        .find(|(_, h)| is_projector_for(h, name.as_deref()))
        .map(|(p, _)| p);
    Ok(LlmFiles { model, mmproj })
}

/// Header checks that do not need the runtime.
pub fn probe(model: &Path) -> Result<LlmProbe, RuntimeError> {
    if !model.is_file() {
        return Err(RuntimeError::ModelMissing);
    }
    let h = read_header(model)?;
    if let Some(e) = kind_error(&h) {
        return Err(RuntimeError::Incompatible(e));
    }
    let arch = h.architecture().unwrap_or_default().to_string();
    Ok(LlmProbe {
        name: h.str("general.name").map(str::to_owned),
        precision: file_type_name(h.u64("general.file_type").unwrap_or(u64::MAX)),
        size_label: h.str("general.size_label").map(str::to_owned),
        context_length: h.u64(&format!("{arch}.context_length")),
        license: h.str("general.license").map(str::to_owned),
        architecture: arch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::build::{header, V};

    fn write(dir: &Path, name: &str, kv: &[(&str, V)]) -> PathBuf {
        let p = dir.join(name);
        let mut bytes = header(kv);
        bytes.extend(std::iter::repeat_n(0u8, name.len() * 100));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn accepts_instruction_models_only() {
        let d = tempfile::tempdir().unwrap();
        let chat = write(
            d.path(),
            "chat-model.gguf",
            &[
                ("general.architecture", V::Str("gemma4")),
                ("general.type", V::Str("model")),
                ("general.name", V::Str("gemma-4-E2B-it")),
                ("general.file_type", V::U32(2)),
                ("gemma4.context_length", V::U32(131072)),
                ("tokenizer.chat_template", V::Str("{{ messages }}")),
            ],
        );
        let p = probe(&chat).unwrap();
        assert_eq!(p.architecture, "gemma4");
        assert_eq!(p.precision, "Q4_0");
        assert_eq!(p.context_length, Some(131072));

        let base = write(
            d.path(),
            "base.gguf",
            &[("general.architecture", V::Str("gemma4"))],
        );
        assert!(matches!(probe(&base), Err(RuntimeError::Incompatible(_))));

        let emb = write(
            d.path(),
            "emb.gguf",
            &[
                ("general.architecture", V::Str("gemma-embedding2")),
                ("tokenizer.chat_template", V::Str("x")),
            ],
        );
        assert!(matches!(probe(&emb), Err(RuntimeError::Incompatible(_))));

        let proj = write(
            d.path(),
            "a-very-long-projector-name-so-it-is-the-largest-file.gguf",
            &[
                ("general.architecture", V::Str("clip")),
                ("general.type", V::Str("mmproj")),
            ],
        );
        assert!(probe(&proj).is_err());
        // Folder: the projector is bigger but is not the model; it belongs
        // to another model name, so it is not attached either.
        let f = locate(d.path()).unwrap();
        assert_eq!(f.model, chat);
        assert_eq!(f.mmproj, None);
        let own = write(
            d.path(),
            "mmproj-own.gguf",
            &[
                ("general.architecture", V::Str("clip")),
                ("general.type", V::Str("mmproj")),
                ("general.name", V::Str("gemma-4-E2B-it")),
                ("clip.has_vision_encoder", V::Bool(true)),
            ],
        );
        let f = locate(d.path()).unwrap();
        assert_eq!(f.mmproj, Some(own.clone()));
        // Picking the model file directly finds the projector too.
        assert_eq!(locate(&chat).unwrap().mmproj, Some(own));
        assert!(matches!(locate(&base), Err(RuntimeError::Incompatible(_))));
    }

    #[test]
    fn missing_and_pins() {
        assert_eq!(
            locate(Path::new("/definitely/not/here")),
            Err(RuntimeError::ModelMissing)
        );
        assert!(is_pinned(
            "8E30DFF3AC4C8434C49A7036FA15564BDBB6044E42BF04550BF1A096AD7E6A52"
        ));
        assert!(!is_pinned("00"));
    }
}
