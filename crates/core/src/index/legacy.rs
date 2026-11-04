//! Legacy binary Office files (Office 97–2003): `.doc`, `.xls`, `.ppt`.
//!
//! All three are OLE compound files (`cfb`). Text is read without running
//! anything from the file:
//! - **.doc**: the piece table (FIB → CLX → PlcPcd) maps character runs to
//!   8-bit (cp1252) or UTF-16 text in the `WordDocument` stream; field
//!   instructions are dropped and field results kept.
//! - **.xls**: cell values of every sheet (`calamine`).
//! - **.ppt**: text atoms (`TextCharsAtom`, `TextBytesAtom`) in the
//!   `PowerPoint Document` stream.
//!
//! Encrypted (password-protected) files are refused.

use std::io::{Cursor, Read};

use crate::index::media::MediaError;
use crate::index::text::cp1252;

const MAX_STREAM: u64 = 64 * 1024 * 1024;
const MAX_CHARS: usize = 4_000_000;

fn stream(c: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut s = c.open_stream(name).ok()?;
    if s.len() > MAX_STREAM {
        return None;
    }
    let mut v = Vec::new();
    s.read_to_end(&mut v).ok()?;
    Some(v)
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Word special characters → plain text; field codes: keep only results.
fn clean_word(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    // Per open field: are we still in its instruction part?
    let mut fields: Vec<bool> = Vec::new();
    for ch in raw.chars() {
        match ch {
            '\u{13}' => fields.push(true),
            '\u{14}' => {
                if let Some(f) = fields.last_mut() {
                    *f = false;
                }
            }
            '\u{15}' => {
                fields.pop();
            }
            _ if fields.iter().any(|in_instr| *in_instr) => {}
            '\r' | '\u{0B}' | '\u{0C}' => out.push('\n'),
            '\u{07}' => out.push('\t'),
            '\t' | '\n' => out.push(ch),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

pub fn doc_text(bytes: &[u8]) -> Result<String, MediaError> {
    let mut c = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| MediaError::Corrupt)?;
    let wd = stream(&mut c, "WordDocument").ok_or(MediaError::Corrupt)?;
    if u16_at(&wd, 0) != Some(0xA5EC) {
        return Err(MediaError::Corrupt);
    }
    let flags = u16_at(&wd, 0x0A).ok_or(MediaError::Corrupt)?;
    if flags & 0x0100 != 0 {
        // fEncrypted
        return Err(MediaError::UnsupportedCodec);
    }
    let table = if flags & 0x0200 != 0 {
        "1Table"
    } else {
        "0Table"
    };
    let tbl = stream(&mut c, table).ok_or(MediaError::Corrupt)?;

    // FIB: base (32) · csw + fibRgW · cslw + fibRgLw · cbRgFcLcb + pairs.
    let csw = u16_at(&wd, 32).ok_or(MediaError::Corrupt)? as usize;
    let lw = 32 + 2 + csw * 2;
    let cslw = u16_at(&wd, lw).ok_or(MediaError::Corrupt)? as usize;
    let fc_lcb = lw + 2 + cslw * 4 + 2;
    // fcClx/lcbClx is pair 33 of FibRgFcLcb97.
    let fc_clx = u32_at(&wd, fc_lcb + 33 * 8).ok_or(MediaError::Corrupt)? as usize;
    let lcb_clx = u32_at(&wd, fc_lcb + 33 * 8 + 4).ok_or(MediaError::Corrupt)? as usize;
    let clx = tbl
        .get(fc_clx..fc_clx.saturating_add(lcb_clx))
        .ok_or(MediaError::Corrupt)?;

    // Skip Prc entries (0x01), then Pcdt (0x02).
    let mut p = 0usize;
    while clx.get(p) == Some(&0x01) {
        let cb = u16_at(clx, p + 1).ok_or(MediaError::Corrupt)? as usize;
        p += 3 + cb;
    }
    if clx.get(p) != Some(&0x02) {
        return Err(MediaError::Corrupt);
    }
    let lcb = u32_at(clx, p + 1).ok_or(MediaError::Corrupt)? as usize;
    let plc = clx.get(p + 5..p + 5 + lcb).ok_or(MediaError::Corrupt)?;
    if lcb < 4 || !(lcb - 4).is_multiple_of(12) {
        return Err(MediaError::Corrupt);
    }
    let n = (lcb - 4) / 12;
    let mut raw = String::new();
    for i in 0..n {
        let cp0 = u32_at(plc, i * 4).ok_or(MediaError::Corrupt)? as usize;
        let cp1 = u32_at(plc, (i + 1) * 4).ok_or(MediaError::Corrupt)? as usize;
        let len = cp1.saturating_sub(cp0);
        let pcd = (n + 1) * 4 + i * 8;
        let fc = u32_at(plc, pcd + 2).ok_or(MediaError::Corrupt)?;
        let compressed = fc & 0x4000_0000 != 0;
        let off = (fc & 0x3FFF_FFFF) as usize;
        let part = if compressed {
            wd.get(off / 2..off / 2 + len).map(cp1252)
        } else {
            wd.get(off..off + len * 2).map(utf16le)
        };
        raw.push_str(&part.ok_or(MediaError::Corrupt)?);
        if raw.len() > MAX_CHARS {
            break;
        }
    }
    let text = clean_word(&raw);
    if text.trim().is_empty() {
        return Err(MediaError::Empty);
    }
    Ok(text)
}

pub fn ppt_text(bytes: &[u8]) -> Result<String, MediaError> {
    let mut c = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| MediaError::Corrupt)?;
    if c.exists("EncryptedSummary") {
        return Err(MediaError::UnsupportedCodec);
    }
    let s = stream(&mut c, "PowerPoint Document").ok_or(MediaError::Corrupt)?;
    let mut out: Vec<String> = Vec::new();
    let mut p = 0usize;
    while p + 8 <= s.len() {
        let ver_inst = u16_at(&s, p).unwrap_or(0);
        let kind = u16_at(&s, p + 2).unwrap_or(0);
        let len = u32_at(&s, p + 4).unwrap_or(0) as usize;
        if ver_inst & 0x000F == 0x000F {
            // Container: its children follow directly.
            p += 8;
            continue;
        }
        let body = s.get(p + 8..(p + 8).saturating_add(len));
        let Some(body) = body else { break };
        let t = match kind {
            0x0FA0 => Some(utf16le(body)),
            0x0FA8 => Some(cp1252(body)),
            _ => None,
        };
        if let Some(t) = t {
            let t = t.replace(['\r', '\u{0B}'], "\n");
            let t = t.trim();
            // Layout placeholders of the slide masters ("*", "Click to edit…").
            if t.chars().any(char::is_alphanumeric) && !t.starts_with("Click to edit") {
                out.push(t.to_string());
            }
        }
        p += 8 + len;
        if out.iter().map(String::len).sum::<usize>() > MAX_CHARS {
            break;
        }
    }
    if out.is_empty() {
        return Err(MediaError::Empty);
    }
    Ok(out.join("\n\n"))
}

pub fn xls_text(bytes: &[u8]) -> Result<String, MediaError> {
    use calamine::{Reader, Xls};
    let mut wb: Xls<_> = calamine::open_workbook_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|_| MediaError::Corrupt)?;
    let mut out = String::new();
    for name in wb.sheet_names().to_vec() {
        let Ok(range) = wb.worksheet_range(&name) else {
            continue;
        };
        out.push_str(&format!("# {name}\n"));
        for row in range.rows().take(20_000) {
            let cells: Vec<String> = row
                .iter()
                .map(|c| c.to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if !cells.is_empty() {
                out.push_str(&cells.join("\t"));
                out.push('\n');
            }
            if out.len() > MAX_CHARS {
                break;
            }
        }
        out.push('\n');
    }
    if out
        .lines()
        .all(|l| l.starts_with("# ") || l.trim().is_empty())
    {
        return Err(MediaError::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn reads_word_97_text_with_turkish_letters() {
        let t = doc_text(&fixture("legacy.doc")).unwrap();
        assert!(t.contains("kira artışı yüzde yirmi beş"), "{t:?}");
        assert!(t.contains("warehouse inventory"));
    }

    #[test]
    fn reads_excel_97_cells() {
        let t = xls_text(&fixture("stock.xls")).unwrap();
        assert!(t.contains("Vida M4\t1200"), "{t:?}");
        assert!(t.contains("Ürün"));
    }

    /// A minimal .ppt built in memory: a container with two text atoms.
    fn tiny_ppt(texts: &[&str]) -> Vec<u8> {
        let mut atoms = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            let (kind, body): (u16, Vec<u8>) = if i % 2 == 0 {
                (
                    0x0FA0,
                    t.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
                )
            } else {
                (0x0FA8, t.bytes().collect())
            };
            atoms.extend(0u16.to_le_bytes());
            atoms.extend(kind.to_le_bytes());
            atoms.extend((body.len() as u32).to_le_bytes());
            atoms.extend(body);
        }
        let mut doc = Vec::new();
        doc.extend(0x000Fu16.to_le_bytes()); // container
        doc.extend(0x03E8u16.to_le_bytes()); // DocumentContainer
        doc.extend((atoms.len() as u32).to_le_bytes());
        doc.extend(atoms);
        let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            use std::io::Write;
            let mut s = c.create_stream("PowerPoint Document").unwrap();
            s.write_all(&doc).unwrap();
        }
        c.into_inner().into_inner()
    }

    #[test]
    fn reads_powerpoint_97_text_atoms() {
        let t = ppt_text(&tiny_ppt(&[
            "Çeyrek sonuçları",
            "Quarterly roadmap",
            "Click to edit Master title style",
            "*",
        ]))
        .unwrap();
        assert_eq!(t, "Çeyrek sonuçları\n\nQuarterly roadmap");
    }

    #[test]
    fn garbage_is_refused() {
        assert!(doc_text(b"not ole").is_err());
        assert!(ppt_text(b"not ole").is_err());
        assert!(xls_text(b"not ole").is_err());
        // An OLE file without the Word stream.
        assert!(doc_text(&tiny_ppt(&["x"])).is_err());
    }

    #[test]
    fn field_instructions_are_dropped() {
        assert_eq!(
            clean_word("See \u{13} HYPERLINK \"x\" \u{14}the site\u{15}.\rNext\u{07}cell"),
            "See the site.\nNext\tcell"
        );
    }
}
