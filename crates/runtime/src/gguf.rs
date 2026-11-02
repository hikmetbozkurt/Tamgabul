//! Minimal, read-only GGUF header reader (docs/04 "cheap static probe").
//!
//! Reads the magic, version and the key/value metadata only. Tensor data is
//! never touched, so probing a 1 GB file reads a few MB at most (the
//! tokenizer arrays are skipped, not loaded). Malformed or hostile files fail
//! with an error instead of allocating unbounded memory.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::RuntimeError;

const MAGIC: &[u8; 4] = b"GGUF";
/// Upper bounds that real files stay far below. They only stop a corrupt
/// header from making us allocate or loop forever.
const MAX_KV: u64 = 1 << 16;
const MAX_STRING: u64 = 64 << 20;
const MAX_ARRAY: u64 = 1 << 26;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    U64(u64),
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(String),
    /// Arrays are not materialized; only element type and length are kept.
    Array {
        elem_type: u32,
        len: u64,
    },
}

impl Value {
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::U64(v) => Some(v),
            Value::I64(v) if v >= 0 => Some(v as u64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GgufHeader {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata: BTreeMap<String, Value>,
}

impl GgufHeader {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
    pub fn u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(Value::as_u64)
    }
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }
    pub fn architecture(&self) -> Option<&str> {
        self.str("general.architecture")
    }
}

fn bad(msg: impl Into<String>) -> RuntimeError {
    RuntimeError::InvalidModelFile(msg.into())
}

struct Reader<R> {
    r: R,
}

impl<R: Read> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], RuntimeError> {
        let mut b = [0u8; N];
        self.r
            .read_exact(&mut b)
            .map_err(|e| bad(format!("truncated header: {e}")))?;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, RuntimeError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, RuntimeError> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    /// Skip by reading (keeps the BufReader's buffer; seeking would drop
    /// it on every one of the ~800k tokenizer strings).
    fn skip(&mut self, n: u64) -> Result<(), RuntimeError> {
        let copied = std::io::copy(&mut (&mut self.r).take(n), &mut std::io::sink())
            .map_err(|e| bad(format!("read: {e}")))?;
        if copied != n {
            return Err(bad("truncated header"));
        }
        Ok(())
    }
    fn string(&mut self) -> Result<String, RuntimeError> {
        let len = self.u64()?;
        if len > MAX_STRING {
            return Err(bad("string length out of range"));
        }
        let mut buf = vec![0u8; len as usize];
        self.r
            .read_exact(&mut buf)
            .map_err(|e| bad(format!("truncated string: {e}")))?;
        String::from_utf8(buf).map_err(|_| bad("string is not UTF-8"))
    }

    /// Fixed size of a scalar GGUF type, `None` for string/array.
    fn scalar_size(t: u32) -> Option<u64> {
        match t {
            0 | 1 | 7 => Some(1), // u8, i8, bool
            2 | 3 => Some(2),     // u16, i16
            4..=6 => Some(4),     // u32, i32, f32
            10..=12 => Some(8),   // u64, i64, f64
            _ => None,
        }
    }

    fn value(&mut self, t: u32) -> Result<Value, RuntimeError> {
        Ok(match t {
            0 => Value::U64(self.bytes::<1>()?[0] as u64),
            1 => Value::I64(self.bytes::<1>()?[0] as i8 as i64),
            2 => Value::U64(u16::from_le_bytes(self.bytes()?) as u64),
            3 => Value::I64(i16::from_le_bytes(self.bytes()?) as i64),
            4 => Value::U64(self.u32()? as u64),
            5 => Value::I64(i32::from_le_bytes(self.bytes()?) as i64),
            6 => Value::F64(f32::from_le_bytes(self.bytes()?) as f64),
            7 => Value::Bool(self.bytes::<1>()?[0] != 0),
            8 => Value::Str(self.string()?),
            9 => {
                let elem_type = self.u32()?;
                let len = self.u64()?;
                if len > MAX_ARRAY {
                    return Err(bad("array length out of range"));
                }
                match (elem_type, Self::scalar_size(elem_type)) {
                    (_, Some(size)) => self.skip(size * len)?,
                    (8, None) => {
                        for _ in 0..len {
                            let n = self.u64()?;
                            if n > MAX_STRING {
                                return Err(bad("string length out of range"));
                            }
                            self.skip(n)?;
                        }
                    }
                    _ => return Err(bad(format!("unsupported array element type {elem_type}"))),
                }
                Value::Array { elem_type, len }
            }
            10 => Value::U64(self.u64()?),
            11 => Value::I64(i64::from_le_bytes(self.bytes()?)),
            12 => Value::F64(f64::from_le_bytes(self.bytes()?)),
            other => return Err(bad(format!("unknown value type {other}"))),
        })
    }
}

pub fn read_header_from<R: Read>(r: R) -> Result<GgufHeader, RuntimeError> {
    let mut rd = Reader { r };
    if &rd.bytes::<4>()? != MAGIC {
        return Err(bad("not a GGUF file"));
    }
    let version = rd.u32()?;
    if !(2..=3).contains(&version) {
        return Err(bad(format!("unsupported GGUF version {version}")));
    }
    let tensor_count = rd.u64()?;
    let kv_count = rd.u64()?;
    if kv_count > MAX_KV {
        return Err(bad("metadata count out of range"));
    }
    let mut metadata = BTreeMap::new();
    for _ in 0..kv_count {
        let key = rd.string()?;
        let t = rd.u32()?;
        let v = rd.value(t)?;
        metadata.insert(key, v);
    }
    Ok(GgufHeader {
        version,
        tensor_count,
        metadata,
    })
}

pub fn read_header(path: &Path) -> Result<GgufHeader, RuntimeError> {
    let f = File::open(path).map_err(|e| RuntimeError::Io(format!("open model file: {e}")))?;
    read_header_from(BufReader::with_capacity(1 << 16, f))
}

/// Test helper for building GGUF headers in memory. Public so integration
/// tests can write small fixture files; it never touches real weights.
pub mod build {
    pub enum V<'a> {
        U32(u32),
        Bool(bool),
        Str(&'a str),
        StrArray(&'a [&'a str]),
        F32Array(&'a [f32]),
    }

    pub fn header(kv: &[(&str, V)]) -> Vec<u8> {
        let mut b = Vec::new();
        let s = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u64).to_le_bytes());
            b.extend(s.as_bytes());
        };
        b.extend(b"GGUF");
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend((kv.len() as u64).to_le_bytes());
        for (k, v) in kv {
            s(&mut b, k);
            match v {
                V::U32(x) => {
                    b.extend(4u32.to_le_bytes());
                    b.extend(x.to_le_bytes());
                }
                V::Bool(x) => {
                    b.extend(7u32.to_le_bytes());
                    b.push(*x as u8);
                }
                V::Str(x) => {
                    b.extend(8u32.to_le_bytes());
                    s(&mut b, x);
                }
                V::StrArray(xs) => {
                    b.extend(9u32.to_le_bytes());
                    b.extend(8u32.to_le_bytes());
                    b.extend((xs.len() as u64).to_le_bytes());
                    for x in *xs {
                        s(&mut b, x);
                    }
                }
                V::F32Array(xs) => {
                    b.extend(9u32.to_le_bytes());
                    b.extend(6u32.to_le_bytes());
                    b.extend((xs.len() as u64).to_le_bytes());
                    for x in *xs {
                        b.extend(x.to_le_bytes());
                    }
                }
            }
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::build::{header, V};
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_scalars_and_skips_arrays() {
        let bytes = header(&[
            ("general.architecture", V::Str("gemma-embedding2")),
            ("x.embedding_length_out", V::U32(768)),
            ("x.causal", V::Bool(false)),
            ("tokenizer.ggml.tokens", V::StrArray(&["a", "bb", "ccc"])),
            ("x.mean", V::F32Array(&[0.5, 0.25])),
            ("after", V::Str("still read")),
        ]);
        let h = read_header_from(Cursor::new(bytes)).unwrap();
        assert_eq!(h.version, 3);
        assert_eq!(h.architecture(), Some("gemma-embedding2"));
        assert_eq!(h.u64("x.embedding_length_out"), Some(768));
        assert_eq!(h.bool("x.causal"), Some(false));
        assert_eq!(
            h.get("tokenizer.ggml.tokens"),
            Some(&Value::Array {
                elem_type: 8,
                len: 3
            })
        );
        assert_eq!(h.str("after"), Some("still read"));
    }

    #[test]
    fn rejects_non_gguf_and_truncated() {
        assert!(matches!(
            read_header_from(Cursor::new(b"PK\x03\x04aaaaaaaa".to_vec())),
            Err(RuntimeError::InvalidModelFile(_))
        ));
        let mut bytes = header(&[("k", V::Str("value"))]);
        bytes.truncate(bytes.len() - 3);
        assert!(matches!(
            read_header_from(Cursor::new(bytes)),
            Err(RuntimeError::InvalidModelFile(_))
        ));
    }

    #[test]
    fn hostile_lengths_do_not_allocate() {
        let mut b = Vec::new();
        b.extend(b"GGUF");
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes()); // key length
        assert!(read_header_from(Cursor::new(b)).is_err());
    }
}
