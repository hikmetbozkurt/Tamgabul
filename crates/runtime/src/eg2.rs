//! EmbeddingGemma 2 profile (docs/04 "EmbeddingGemma 2 advanced details").
//!
//! Static validation of the GGUF pair (text model + optional mmproj with the
//! vision/audio encoders), the official task prompts, and vector helpers
//! (L2 normalization, Matryoshka truncation, cosine).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::RuntimeError;
use crate::gguf::{read_header, GgufHeader};

pub const ARCH: &str = "gemma-embedding2";
pub const NATIVE_DIM: usize = 768;
pub const MATRYOSHKA_DIMS: [usize; 4] = [768, 512, 256, 128];

/// Files published at `ggml-org/embeddinggemma-2-GGUF`, revision
/// `6ea3e3a309d12a4779cfda98b8c19800d2d5da71` (LFS sha256 from the Hub).
/// A different hash is allowed (own conversion) but reported as unpinned.
pub const PINNED_REVISION: &str =
    "ggml-org/embeddinggemma-2-GGUF@6ea3e3a309d12a4779cfda98b8c19800d2d5da71";
pub const PINNED_SHA256: &[(&str, &str)] = &[
    (
        "embeddinggemma-2-BF16.gguf",
        "68bae29d62fb8c7d23e98d21fd4662753ddd636e6b62b8a70dfe059a9844f216",
    ),
    (
        "embeddinggemma-2-Q8_0.gguf",
        "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135",
    ),
    (
        "mmproj-embeddinggemma-2-BF16.gguf",
        "cc8d7eb30a9984c5128d6b1ccdec7510bfce213dc92a840be0d20e2a62b26bcd",
    ),
    (
        "mmproj-embeddinggemma-2-Q8_0.gguf",
        "0531309472a6bc6f4b146c3736eee7795ec9db1903c25490d3c937490cef6dd8",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eg2Files {
    pub model: PathBuf,
    /// Vision + audio encoders. Without it only text embeddings work.
    pub mmproj: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eg2Probe {
    pub output_dim: usize,
    /// GGUF `general.file_type` name, e.g. "BF16" or "Q8_0".
    pub precision: String,
    pub size_label: Option<String>,
    pub vision: bool,
    pub audio: bool,
}

pub fn file_type_name(ft: u64) -> String {
    match ft {
        0 => "F32".into(),
        1 => "F16".into(),
        2 => "Q4_0".into(),
        3 => "Q4_1".into(),
        7 => "Q8_0".into(),
        8 => "Q5_0".into(),
        9 => "Q5_1".into(),
        10 => "Q2_K".into(),
        11 => "Q3_K_S".into(),
        12 => "Q3_K_M".into(),
        13 => "Q3_K_L".into(),
        14 => "Q4_K_S".into(),
        15 => "Q4_K_M".into(),
        16 => "Q5_K_S".into(),
        17 => "Q5_K_M".into(),
        18 => "Q6_K".into(),
        32 => "BF16".into(),
        other => format!("type {other}"),
    }
}

fn is_mmproj(h: &GgufHeader) -> bool {
    h.str("general.type") == Some("mmproj") || h.architecture() == Some("clip")
}

/// Find the EG2 model and mmproj inside a folder by reading GGUF headers
/// (never by trusting file names). Accepts a file path too.
pub fn locate(path: &Path) -> Result<Eg2Files, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::ModelMissing);
    }
    let candidates: Vec<PathBuf> = if path.is_file() {
        let mut v = vec![path.to_path_buf()];
        if let Some(dir) = path.parent() {
            v.extend(gguf_files(dir)?.into_iter().filter(|p| p != path));
        }
        v
    } else {
        gguf_files(path)?
    };
    let mut model = None;
    let mut mmproj = None;
    for p in candidates {
        let Ok(h) = read_header(&p) else { continue };
        if is_mmproj(&h) {
            if mmproj.is_none()
                && h.str("general.name")
                    .is_some_and(|n| n.contains("embeddinggemma"))
            {
                mmproj = Some(p);
            }
        } else if model.is_none() && h.architecture() == Some(ARCH) {
            model = Some(p);
        }
    }
    match model {
        Some(model) => Ok(Eg2Files { model, mmproj }),
        None => Err(RuntimeError::Incompatible(format!(
            "no GGUF with architecture '{ARCH}' found"
        ))),
    }
}

fn gguf_files(dir: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let rd = std::fs::read_dir(dir).map_err(|e| RuntimeError::Io(format!("read folder: {e}")))?;
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
        })
        .collect();
    v.sort();
    Ok(v)
}

/// Header checks that do not need the runtime. Cheap: reads metadata only.
pub fn probe(files: &Eg2Files) -> Result<Eg2Probe, RuntimeError> {
    if !files.model.is_file() {
        return Err(RuntimeError::ModelMissing);
    }
    let h = read_header(&files.model)?;
    if h.architecture() != Some(ARCH) {
        return Err(RuntimeError::Incompatible(format!(
            "architecture is '{}', expected '{ARCH}'",
            h.architecture().unwrap_or("unknown")
        )));
    }
    let out = h
        .u64(&format!("{ARCH}.embedding_length_out"))
        .ok_or_else(|| RuntimeError::Incompatible("embedding_length_out missing".into()))?;
    if out as usize != NATIVE_DIM {
        return Err(RuntimeError::Incompatible(format!(
            "output dimension {out}, expected {NATIVE_DIM}"
        )));
    }
    if h.bool(&format!("{ARCH}.attention.causal")) == Some(true) {
        return Err(RuntimeError::Incompatible(
            "model is causal, expected bidirectional encoder".into(),
        ));
    }
    let hidden = h.u64(&format!("{ARCH}.embedding_length"));
    let mut probe = Eg2Probe {
        output_dim: NATIVE_DIM,
        precision: file_type_name(h.u64("general.file_type").unwrap_or(u64::MAX)),
        size_label: h.str("general.size_label").map(str::to_owned),
        vision: false,
        audio: false,
    };
    if let Some(mm) = &files.mmproj {
        if !mm.is_file() {
            return Err(RuntimeError::ModelMissing);
        }
        let m = read_header(mm)?;
        if !is_mmproj(&m) {
            return Err(RuntimeError::Incompatible(
                "projector file is not an mmproj".into(),
            ));
        }
        // Encoders must project into the text model's hidden size.
        for (flag, dim_key, set) in [
            (
                "clip.has_vision_encoder",
                "clip.vision.projection_dim",
                &mut probe.vision,
            ),
            (
                "clip.has_audio_encoder",
                "clip.audio.projection_dim",
                &mut probe.audio,
            ),
        ] {
            if m.bool(flag) == Some(true) {
                let pd = m.u64(dim_key);
                if hidden.is_some() && pd.is_some() && pd != hidden {
                    return Err(RuntimeError::Incompatible(format!(
                        "{dim_key}={} does not match model width {}",
                        pd.unwrap_or(0),
                        hidden.unwrap_or(0)
                    )));
                }
                *set = true;
            }
        }
    }
    Ok(probe)
}

/// SHA-256 of a file, streamed. `progress(done, total)` is called per MiB.
pub fn sha256_file(
    path: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<String, RuntimeError> {
    let f = File::open(path).map_err(|e| RuntimeError::Io(format!("open: {e}")))?;
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = r
            .read(&mut buf)
            .map_err(|e| RuntimeError::Io(format!("read: {e}")))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `true` if the hash matches a file of the pinned upstream revision.
pub fn is_pinned(sha256: &str) -> bool {
    PINNED_SHA256
        .iter()
        .any(|(_, h)| h.eq_ignore_ascii_case(sha256))
}

/// Official task prompts from the model card. Queries and documents use
/// different prefixes; media is passed without any prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    SearchResult,
    QuestionAnswering,
    CodeRetrieval,
}

impl Task {
    fn name(self) -> &'static str {
        match self {
            Task::SearchResult => "search result",
            Task::QuestionAnswering => "question answering",
            Task::CodeRetrieval => "code retrieval",
        }
    }
}

pub fn query_prompt(task: Task, query: &str) -> String {
    format!("task: {} | query: {query}", task.name())
}

pub fn document_prompt(title: Option<&str>, text: &str) -> String {
    let title = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("none");
    format!("title: {title} | text: {text}")
}

/// L2-normalize in place. A zero vector stays zero (and is reported invalid
/// by [`is_valid_vector`]).
pub fn normalize(v: &mut [f32]) {
    let n = v
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x = (*x as f64 / n) as f32;
        }
    }
}

/// Matryoshka truncation: keep the first `dim` values and re-normalize.
pub fn truncate(v: &[f32], dim: usize) -> Result<Vec<f32>, RuntimeError> {
    if !MATRYOSHKA_DIMS.contains(&dim) || v.len() < dim {
        return Err(RuntimeError::BadResponse(format!(
            "cannot truncate {} values to {dim}",
            v.len()
        )));
    }
    let mut out = v[..dim].to_vec();
    normalize(&mut out);
    Ok(out)
}

pub fn is_valid_vector(v: &[f32]) -> bool {
    !v.is_empty() && v.iter().all(|x| x.is_finite()) && v.iter().any(|x| *x != 0.0)
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut d, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        d += (*x as f64) * (*y as f64);
        na += (*x as f64) * (*x as f64);
        nb += (*y as f64) * (*y as f64);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        (d / (na.sqrt() * nb.sqrt())) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::build::{header, V};

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn model_header(arch: &str, out: u32) -> Vec<u8> {
        header(&[
            ("general.architecture", V::Str(arch)),
            ("general.type", V::Str("model")),
            ("general.name", V::Str("embeddinggemma-2")),
            ("general.file_type", V::U32(32)),
            ("gemma-embedding2.embedding_length", V::U32(512)),
            ("gemma-embedding2.embedding_length_out", V::U32(out)),
            ("gemma-embedding2.attention.causal", V::Bool(false)),
        ])
    }

    fn mmproj_header(proj: u32) -> Vec<u8> {
        header(&[
            ("general.architecture", V::Str("clip")),
            ("general.type", V::Str("mmproj")),
            ("general.name", V::Str("embeddinggemma-2")),
            ("clip.has_vision_encoder", V::Bool(true)),
            ("clip.vision.projection_dim", V::U32(proj)),
            ("clip.has_audio_encoder", V::Bool(true)),
            ("clip.audio.projection_dim", V::U32(proj)),
        ])
    }

    #[test]
    fn locates_pair_by_header_not_name() {
        let d = tempfile::tempdir().unwrap();
        // Deliberately misleading names.
        let m = write(d.path(), "b.gguf", &model_header(ARCH, 768));
        let p = write(d.path(), "a.gguf", &mmproj_header(512));
        write(d.path(), "notes.txt", b"x");
        let f = locate(d.path()).unwrap();
        assert_eq!(f.model, m);
        assert_eq!(f.mmproj.as_deref(), Some(p.as_path()));
        let pr = probe(&f).unwrap();
        assert_eq!(pr.output_dim, 768);
        assert_eq!(pr.precision, "BF16");
        assert!(pr.vision && pr.audio);
        // Selecting the model file directly finds the sibling mmproj too.
        assert_eq!(locate(&m).unwrap().mmproj.as_deref(), Some(p.as_path()));
    }

    #[test]
    fn text_only_when_no_mmproj() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "m.gguf", &model_header(ARCH, 768));
        let f = locate(d.path()).unwrap();
        assert_eq!(f.mmproj, None);
        let pr = probe(&f).unwrap();
        assert!(!pr.vision && !pr.audio);
    }

    #[test]
    fn wrong_architecture_or_dim_is_incompatible() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "llm.gguf", &model_header("gemma3", 768));
        assert!(matches!(
            locate(d.path()),
            Err(RuntimeError::Incompatible(_))
        ));

        let m = write(d.path(), "eg.gguf", &model_header(ARCH, 1024));
        let f = Eg2Files {
            model: m,
            mmproj: None,
        };
        assert!(matches!(probe(&f), Err(RuntimeError::Incompatible(_))));
    }

    #[test]
    fn mismatched_projector_is_incompatible() {
        let d = tempfile::tempdir().unwrap();
        let f = Eg2Files {
            model: write(d.path(), "m.gguf", &model_header(ARCH, 768)),
            mmproj: Some(write(d.path(), "p.gguf", &mmproj_header(1024))),
        };
        assert!(matches!(probe(&f), Err(RuntimeError::Incompatible(_))));
    }

    #[test]
    fn missing_path_is_missing() {
        assert_eq!(
            locate(Path::new("/definitely/not/here")),
            Err(RuntimeError::ModelMissing)
        );
    }

    #[test]
    fn prompts_match_model_card() {
        assert_eq!(
            query_prompt(Task::SearchResult, "auction timer bug"),
            "task: search result | query: auction timer bug"
        );
        assert_eq!(document_prompt(None, "body"), "title: none | text: body");
        assert_eq!(document_prompt(Some("  "), "b"), "title: none | text: b");
        assert_eq!(
            document_prompt(Some("notes.md"), "b"),
            "title: notes.md | text: b"
        );
    }

    #[test]
    fn truncation_renormalizes() {
        let mut v: Vec<f32> = (1..=768).map(|i| i as f32).collect();
        normalize(&mut v);
        let t = truncate(&v, 256).unwrap();
        assert_eq!(t.len(), 256);
        let n: f32 = t.iter().map(|x| x * x).sum();
        assert!((n - 1.0).abs() < 1e-5);
        assert!(truncate(&v, 300).is_err());
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
        assert!(!is_valid_vector(&[0.0, 0.0]));
        assert!(!is_valid_vector(&[f32::NAN, 1.0]));
    }

    #[test]
    fn sha256_and_pin() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "x", b"abc");
        let h = sha256_file(&p, |_, _| {}).unwrap();
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(!is_pinned(&h));
        assert!(is_pinned(PINNED_SHA256[0].1));
    }
}
