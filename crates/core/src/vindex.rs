//! In-memory vector index for one index generation (docs/03 retrieval).
//!
//! Before: every query read every vector blob from SQLite and compared it
//! in full precision. Now the vectors of the current generation are loaded
//! once into one contiguous int8 matrix (768 bytes per chunk instead of
//! 3 KiB of f32, plus one scale per row), scanned in parallel, and the best
//! candidates are re-scored exactly from SQLite. The cache is rebuilt when
//! the generation's vectors change (a cheap integer signature, no blobs).
//!
//! The index is derived data: losing it costs one rebuild, never results.

use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::{params, Connection};

use crate::error::{ErrorCode, IpcError};
use crate::index::store;

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("vector index: {e}"))
}

/// One row: chunk, its document and modality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub chunk_id: i64,
    pub document_id: Arc<str>,
    pub modality: Arc<str>,
}

pub struct VectorIndex {
    /// Database file the index was built from (`None` = in-memory DB).
    db: Option<String>,
    pub generation: String,
    signature: (i64, i64, i64),
    pub dim: usize,
    rows: Vec<Row>,
    /// `rows.len() * dim` int8 values, row-major.
    codes: Vec<i8>,
    /// Per-row dequantization scale (max |x| / 127).
    scales: Vec<f32>,
}

/// Symmetric per-row int8 quantization.
pub fn quantize(v: &[f32]) -> (Vec<i8>, f32) {
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    if max == 0.0 || !max.is_finite() {
        return (vec![0; v.len()], 0.0);
    }
    let s = max / 127.0;
    (
        v.iter()
            .map(|x| (x / s).round().clamp(-127.0, 127.0) as i8)
            .collect(),
        s,
    )
}

fn signature(conn: &Connection, generation: &str) -> Result<(i64, i64, i64), IpcError> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(MAX(chunk_id), 0), COALESCE(SUM(chunk_id), 0)
         FROM embedding_records WHERE generation_id = ?1",
        [generation],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map_err(err)
}

impl VectorIndex {
    pub fn build(conn: &Connection, generation: &str) -> Result<Self, IpcError> {
        let signature = signature(conn, generation)?;
        let mut st = conn
            .prepare(
                "SELECT e.chunk_id, c.document_id, c.modality, e.vector FROM embedding_records e
                 JOIN chunks c ON c.chunk_id = e.chunk_id WHERE e.generation_id = ?1
                 ORDER BY e.chunk_id",
            )
            .map_err(err)?;
        let mut q = st.query([generation]).map_err(err)?;
        let mut idx = VectorIndex {
            db: conn.path().map(str::to_owned),
            generation: generation.to_string(),
            signature,
            dim: 0,
            rows: Vec::new(),
            codes: Vec::new(),
            scales: Vec::new(),
        };
        let mut docs: std::collections::HashMap<String, Arc<str>> = Default::default();
        let mut mods: std::collections::HashMap<String, Arc<str>> = Default::default();
        while let Some(r) = q.next().map_err(err)? {
            let blob: Vec<u8> = r.get(3).map_err(err)?;
            let v = store::vector_from_bytes(&blob);
            if idx.dim == 0 {
                idx.dim = v.len();
            }
            if v.len() != idx.dim || v.is_empty() {
                continue;
            }
            let (c, s) = quantize(&v);
            idx.codes.extend_from_slice(&c);
            idx.scales.push(s);
            let d: String = r.get(1).map_err(err)?;
            let m: String = r.get(2).map_err(err)?;
            let document_id = docs.entry(d.clone()).or_insert_with(|| d.into()).clone();
            let modality = mods.entry(m.clone()).or_insert_with(|| m.into()).clone();
            idx.rows.push(Row {
                chunk_id: r.get(0).map_err(err)?,
                document_id,
                modality,
            });
        }
        Ok(idx)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Approximate cosine (int8 · int8) for every row, scanned on a few
    /// threads. Returns the `k` best row indexes, best first.
    fn coarse_top(&self, q: &[f32], k: usize) -> Vec<(usize, f32)> {
        let n = self.rows.len();
        if n == 0 || q.len() != self.dim {
            return Vec::new();
        }
        let threads = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
            .clamp(1, 8)
            .min(n.div_ceil(4096));
        let per = n.div_ceil(threads.max(1));
        // The query is quantized too: i8·i8 sums in i32 vectorize well.
        let (qc, qs) = quantize(q);
        let qc = &qc;
        let mut parts: Vec<Vec<(usize, f32)>> = Vec::new();
        std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads.max(1))
                .map(|t| {
                    let (a, b) = (t * per, ((t + 1) * per).min(n));
                    s.spawn(move || {
                        let mut local: Vec<(usize, f32)> = Vec::with_capacity(b.saturating_sub(a));
                        for i in a..b {
                            let row = &self.codes[i * self.dim..(i + 1) * self.dim];
                            let dot: i32 = row
                                .iter()
                                .zip(qc.iter())
                                .map(|(c, x)| *c as i32 * *x as i32)
                                .sum();
                            local.push((i, dot as f32 * self.scales[i] * qs));
                        }
                        if local.len() > k {
                            local.select_nth_unstable_by(k, |x, y| y.1.total_cmp(&x.1));
                            local.truncate(k);
                        }
                        local
                    })
                })
                .collect();
            parts = hs.into_iter().filter_map(|h| h.join().ok()).collect();
        });
        let mut all: Vec<(usize, f32)> = parts.into_iter().flatten().collect();
        all.sort_by(|x, y| y.1.total_cmp(&x.1));
        all.truncate(k);
        all
    }

    /// Top chunks by exact cosine within each modality: a wide coarse pass
    /// on int8 codes, then exact re-scoring of those from the stored f32
    /// vectors. `per_modality` candidates are kept per modality.
    pub fn search(
        &self,
        conn: &Connection,
        q: &[f32],
        per_modality: usize,
    ) -> Result<Vec<(Row, f32)>, IpcError> {
        // Over-fetch so int8 rounding never pushes a true top hit out.
        let coarse = self.coarse_top(q, (per_modality * 2).max(200));
        let mut st = conn
            .prepare_cached(
                "SELECT vector FROM embedding_records WHERE chunk_id = ?1 AND generation_id = ?2",
            )
            .map_err(err)?;
        let mut out: Vec<(Row, f32)> = Vec::with_capacity(coarse.len());
        for (i, _) in coarse {
            let row = &self.rows[i];
            let blob: Option<Vec<u8>> = st
                .query_row(params![row.chunk_id, self.generation], |r| r.get(0))
                .ok();
            let Some(blob) = blob else { continue };
            let v = store::vector_from_bytes(&blob);
            if v.len() != q.len() {
                continue;
            }
            let s: f32 = v.iter().zip(q).map(|(a, b)| a * b).sum();
            out.push((row.clone(), s));
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut kept: std::collections::HashMap<Arc<str>, usize> = Default::default();
        out.retain(|(r, _)| {
            let n = kept.entry(r.modality.clone()).or_insert(0);
            *n += 1;
            *n <= per_modality
        });
        Ok(out)
    }
}

/// Process-wide cache of the last built index (one generation is current).
fn cache() -> &'static Mutex<Option<Arc<VectorIndex>>> {
    static C: OnceLock<Mutex<Option<Arc<VectorIndex>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// The index for `generation`, rebuilt only when its vectors changed.
pub fn for_generation(conn: &Connection, generation: &str) -> Result<Arc<VectorIndex>, IpcError> {
    let sig = signature(conn, generation)?;
    if let Ok(g) = cache().lock() {
        if let Some(i) = g.as_ref() {
            if i.generation == generation && i.signature == sig && i.db.as_deref() == conn.path() {
                return Ok(i.clone());
            }
        }
    }
    let built = Arc::new(VectorIndex::build(conn, generation)?);
    if let Ok(mut g) = cache().lock() {
        *g = Some(built.clone());
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random unit vectors.
    fn vectors(n: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                let mut v: Vec<f32> = (0..dim)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        (x % 2001) as f32 / 1000.0 - 1.0
                    })
                    .collect();
                let n = v.iter().map(|a| a * a).sum::<f32>().sqrt();
                v.iter_mut().for_each(|a| *a /= n);
                v
            })
            .collect()
    }

    #[test]
    fn same_top_hits_as_exact_brute_force_and_cache_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_and_migrate(&dir.path().join("v.db")).unwrap();
        conn.execute_batch(
            "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version)
                VALUES ('m','embedding','E','x','gguf','l','1');
             INSERT INTO index_generations(generation_id, embedding_model_hash, model_revision, adapter_version,
                preprocessing_pipeline, query_instruction, document_instruction, dimension, normalization,
                modalities_json, fingerprint) VALUES ('g','h','r','a','p','q','d',768,'l2','[]','f');
             INSERT INTO documents(document_id, canonical_path, file_type, size_bytes, mtime) VALUES ('d','/x.md','markdown',1,'0');",
        )
        .unwrap();
        let data = vectors(3000, 768, 42);
        for (i, v) in data.iter().enumerate() {
            let m = if i % 5 == 0 { "image" } else { "text" };
            conn.execute(
                "INSERT INTO chunks(chunk_id, document_id, generation_id, ordinal, modality) VALUES (?1,'d','g',?1,?2)",
                params![i as i64 + 1, m],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO embedding_records(chunk_id, generation_id, dimension, vector) VALUES (?1,'g',768,?2)",
                params![i as i64 + 1, store::vector_bytes(v)],
            )
            .unwrap();
        }
        let idx = for_generation(&conn, "g").unwrap();
        assert_eq!(idx.len(), 3000);
        for q in vectors(20, 768, 7) {
            let got = idx.search(&conn, &q, 10).unwrap();
            for m in ["text", "image"] {
                let mut exact: Vec<(i64, f32)> = data
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| (i % 5 == 0) == (m == "image"))
                    .map(|(i, v)| (i as i64 + 1, v.iter().zip(&q).map(|(a, b)| a * b).sum()))
                    .collect();
                exact.sort_by(|a, b| b.1.total_cmp(&a.1));
                let want: Vec<i64> = exact.iter().take(10).map(|e| e.0).collect();
                let have: Vec<i64> = got
                    .iter()
                    .filter(|(r, _)| &*r.modality == m)
                    .map(|(r, _)| r.chunk_id)
                    .collect();
                assert_eq!(have, want, "{m}");
            }
        }
        // Cached until vectors change; a new vector is seen next time.
        assert!(Arc::ptr_eq(&idx, &for_generation(&conn, "g").unwrap()));
        conn.execute(
            "INSERT INTO chunks(chunk_id, document_id, generation_id, ordinal, modality) VALUES (9999,'d','g',9999,'text')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embedding_records(chunk_id, generation_id, dimension, vector) VALUES (9999,'g',768,?1)",
            [store::vector_bytes(&data[0])],
        )
        .unwrap();
        assert_eq!(for_generation(&conn, "g").unwrap().len(), 3001);
    }

    /// Timing only, not part of CI:
    /// `cargo test -p tamgabul-core --release --lib vindex::tests::timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn timing() {
        let n = 100_000;
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open_and_migrate(&dir.path().join("t.db")).unwrap();
        conn.execute_batch(
            "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version)
                VALUES ('m','embedding','E','x','gguf','l','1');
             INSERT INTO index_generations(generation_id, embedding_model_hash, model_revision, adapter_version,
                preprocessing_pipeline, query_instruction, document_instruction, dimension, normalization,
                modalities_json, fingerprint) VALUES ('g','h','r','a','p','q','d',768,'l2','[]','f');
             INSERT INTO documents(document_id, canonical_path, file_type, size_bytes, mtime) VALUES ('d','/x.md','markdown',1,'0');",
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        for (i, v) in vectors(n, 768, 3).iter().enumerate() {
            tx.execute(
                "INSERT INTO chunks(chunk_id, document_id, generation_id, ordinal, modality) VALUES (?1,'d','g',?1,'text')",
                params![i as i64 + 1],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO embedding_records(chunk_id, generation_id, dimension, vector) VALUES (?1,'g',768,?2)",
                params![i as i64 + 1, store::vector_bytes(v)],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let q = &vectors(1, 768, 9)[0];
        // Old way: read and compare every blob.
        let t = std::time::Instant::now();
        let mut st = conn
            .prepare("SELECT vector FROM embedding_records WHERE generation_id = 'g'")
            .unwrap();
        let mut best = f32::MIN;
        let mut rows = st.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let b: Vec<u8> = r.get(0).unwrap();
            let v = store::vector_from_bytes(&b);
            best = best.max(v.iter().zip(q).map(|(a, b)| a * b).sum());
        }
        let old = t.elapsed();
        let t = std::time::Instant::now();
        let idx = for_generation(&conn, "g").unwrap();
        let build = t.elapsed();
        let t = std::time::Instant::now();
        let hits = idx.search(&conn, q, 200).unwrap();
        let new = t.elapsed();
        assert!((hits[0].1 - best).abs() < 1e-5);
        eprintln!("{n} vectors: full scan {old:?} · build once {build:?} · indexed query {new:?}");
    }

    #[test]
    fn quantization_keeps_direction() {
        let v = [0.5f32, -0.25, 0.0, 1.0];
        let (c, s) = quantize(&v);
        assert_eq!(c[3], 127);
        for (a, b) in c.iter().zip(v) {
            assert!(((*a as f32) * s - b).abs() < 0.01);
        }
        assert_eq!(quantize(&[0.0, 0.0]).1, 0.0);
    }
}
