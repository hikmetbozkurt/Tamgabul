//! Text decoding and chunking with line provenance (docs/07 index pipeline).
//!
//! Decoding: UTF-8 (with or without BOM), UTF-16 LE/BE (BOM or NUL-pattern),
//! and Windows-1254 (Turkish) as the legacy fallback. Files that look binary
//! are refused rather than indexed as noise.

/// Windows-1252/1254 mapping for 0x80..=0x9F (undefined → U+FFFD).
const CP125X_80: [char; 32] = [
    '€', '\u{FFFD}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{FFFD}', '\u{FFFD}',
    '\u{FFFD}', '\u{FFFD}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{FFFD}',
    '\u{FFFD}', 'Ÿ',
];

fn cp1254(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7F => b as char,
            0x80..=0x9F => CP125X_80[(b - 0x80) as usize],
            0xD0 => 'Ğ',
            0xDD => 'İ',
            0xDE => 'Ş',
            0xF0 => 'ğ',
            0xFD => 'ı',
            0xFE => 'ş',
            _ => char::from_u32(b as u32).unwrap_or('\u{FFFD}'),
        })
        .collect()
}

/// Windows-1252 (Western): the 8-bit code page of legacy Office text.
pub fn cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP125X_80[(b - 0x80) as usize],
            _ => b as char,
        })
        .collect()
}

fn utf16(bytes: &[u8], le: bool) -> Option<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| {
            if le {
                u16::from_le_bytes(*c)
            } else {
                u16::from_be_bytes(*c)
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// Decode file bytes to text. `None` = binary / undecodable.
pub fn decode(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).ok();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    // BOM-less UTF-16: many NULs on one byte parity.
    let sample = &bytes[..bytes.len().min(4096)];
    let nul_even = sample.iter().step_by(2).filter(|b| **b == 0).count();
    let nul_odd = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|b| **b == 0)
        .count();
    let half = sample.len() / 2;
    if half >= 8 && nul_odd * 10 >= half * 4 && nul_even * 10 < half {
        return utf16(bytes, true);
    }
    if half >= 8 && nul_even * 10 >= half * 4 && nul_odd * 10 < half {
        return utf16(bytes, false);
    }
    if sample.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_owned()),
        Err(_) => {
            // Mostly-control bytes = binary, not legacy text.
            let ctrl = sample
                .iter()
                .filter(|b| **b < 0x09 || (**b > 0x0D && **b < 0x20))
                .count();
            if ctrl * 20 > sample.len().max(1) {
                None
            } else {
                Some(cp1254(bytes))
            }
        }
    }
}

/// One-to-one character folding for word search: lower case, Turkish
/// `I/İ/ı` → `i`, accents removed (`ş→s`, `ü→u`, `é→e`). Same length in
/// chars as the input, so match positions map back to the original text.
pub fn fold_char(c: char) -> char {
    use unicode_normalization::UnicodeNormalization;
    match c {
        'I' | 'İ' | 'ı' => 'i',
        c if c.is_ascii() => c.to_ascii_lowercase(),
        c => {
            let lower = c.to_lowercase().next().unwrap_or(c);
            lower.nfd().next().unwrap_or(lower)
        }
    }
}

pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub ordinal: u32,
    pub text: String,
    /// 1-based, inclusive.
    pub line_start: u32,
    pub line_end: u32,
}

pub const TARGET_CHARS: usize = 1000;
pub const MAX_CHARS: usize = 1600;
pub const MAX_CHUNKS_PER_DOC: usize = 2000;

/// Split into chunks of about [`TARGET_CHARS`], preferring blank lines and
/// Markdown headings as boundaries. Very long lines are split hard.
pub fn chunk(text: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0u32;
    let mut end = 0u32;

    let flush = |buf: &mut String, start: u32, end: u32, out: &mut Vec<Chunk>| {
        let t = buf.trim();
        if !t.is_empty() && out.len() < MAX_CHUNKS_PER_DOC {
            out.push(Chunk {
                ordinal: out.len() as u32,
                text: t.to_string(),
                line_start: start,
                line_end: end,
            });
        }
        buf.clear();
    };

    for (i, raw) in text.lines().enumerate() {
        let n = i as u32 + 1;
        let line = raw.trim_end_matches('\r');
        if buf.is_empty() && line.trim().is_empty() {
            continue;
        }
        let len = buf.chars().count();
        let heading = line.starts_with('#');
        if !buf.is_empty()
            && ((heading && len >= 200) || (line.trim().is_empty() && len >= TARGET_CHARS * 6 / 10))
        {
            flush(&mut buf, start, end, &mut out);
        }
        // Hard-split lines that alone exceed the max.
        let mut rest: &str = line;
        loop {
            if buf.is_empty() {
                start = n;
            }
            let room = MAX_CHARS.saturating_sub(buf.chars().count());
            if rest.chars().count() <= room {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(rest);
                end = n;
                break;
            }
            if !buf.is_empty() {
                flush(&mut buf, start, end, &mut out);
                continue;
            }
            let cut = rest
                .char_indices()
                .nth(MAX_CHARS)
                .map(|(b, _)| b)
                .unwrap_or(rest.len());
            buf.push_str(&rest[..cut]);
            end = n;
            flush(&mut buf, start, end, &mut out);
            rest = &rest[cut..];
            if rest.is_empty() {
                break;
            }
        }
        if buf.chars().count() >= MAX_CHARS {
            flush(&mut buf, start, end, &mut out);
        }
    }
    flush(&mut buf, start, end, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_utf16_and_turkish_legacy() {
        assert_eq!(decode("şçığ".as_bytes()).unwrap(), "şçığ");
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'h', b'i']).unwrap(), "hi");
        let mut le = vec![0xFF, 0xFE];
        le.extend("Ağaç".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(decode(&le).unwrap(), "Ağaç");
        let be: Vec<u8> = "plain utf16 text here"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(decode(&be).unwrap(), "plain utf16 text here");
        // "İşçi ağı" in Windows-1254
        let legacy = [0xDD, 0xFE, 0xE7, 0x69, 0x20, 0x61, 0xF0, 0xFD];
        assert_eq!(decode(&legacy).unwrap(), "İşçi ağı");
    }

    #[test]
    fn folding_is_turkish_aware_and_length_preserving() {
        assert_eq!(
            fold("Toplantı İŞÇİ Ağaç Über café"),
            "toplanti isci agac uber cafe"
        );
        for s in ["Toplantı", "İstanbul", "ÇĞÖŞÜ", "naïve"] {
            assert_eq!(fold(s).chars().count(), s.chars().count());
        }
    }

    #[test]
    fn binary_is_refused() {
        assert_eq!(
            decode(&[0x89, b'P', b'N', b'G', 0, 0, 0, 13, 1, 2, 3]),
            None
        );
        assert_eq!(decode(&[1, 2, 3, 4, 5, 6, 7, 8, 0xFF, 0xFE, 1, 2]), None);
    }

    #[test]
    fn chunks_keep_line_provenance() {
        let mut text = String::from("# Title\nintro line\n\n");
        for i in 0..60 {
            text.push_str(&format!("paragraph line {i} with some words in it\n"));
            if i % 10 == 9 {
                text.push('\n');
            }
        }
        text.push_str("# Second\nend\n");
        let c = chunk(&text);
        assert!(c.len() >= 2, "{}", c.len());
        assert_eq!(c[0].line_start, 1);
        assert!(c.iter().all(|x| x.text.chars().count() <= MAX_CHARS));
        assert!(c.windows(2).all(|w| w[0].line_end < w[1].line_start));
        assert_eq!(c.last().unwrap().text, "# Second\nend");
        let total = text.lines().count() as u32;
        assert_eq!(c.last().unwrap().line_end, total);
        assert!(c.iter().enumerate().all(|(i, x)| x.ordinal == i as u32));
    }

    #[test]
    fn huge_single_line_is_split() {
        let line = "x".repeat(MAX_CHARS * 2 + 10);
        let c = chunk(&line);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| x.line_start == 1 && x.line_end == 1));
    }

    #[test]
    fn empty_text_has_no_chunks() {
        assert!(chunk("").is_empty());
        assert!(chunk("\n\n  \n").is_empty());
    }
}
