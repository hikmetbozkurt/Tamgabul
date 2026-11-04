//! Turning any supported file into something the embedding model and OCR
//! can read (docs/07 "classify → parse → normalize").
//!
//! - Images: decoded with the portable decoder (PNG, JPEG, GIF, BMP, ICO,
//!   TIFF, WebP, PNM, TGA, DDS, HDR, EXR, QOI) or, for HEIC/HEIF/AVIF/JPEG
//!   XR/RAW, by the OS. Always re-encoded as a bounded PNG so the model
//!   loader only ever sees one well-known format.
//! - Audio: decoded (MP3, AAC/M4A, FLAC, ALAC, Vorbis/OGG, WAV, AIFF, CAF,
//!   MKA/WebM), mixed to mono, resampled to 16 kHz (EmbeddingGemma 2 input)
//!   and cut into fixed segments.
//! - Documents: DOCX/PPTX/XLSX and ODT/ODP/ODS text from their XML parts;
//!   HTML/XML/RTF markup stripped.
//!
//! Everything here is pure and bounded (pixel, duration and size caps), so
//! a hostile file costs at most a skipped entry.

use std::io::{Cursor, Read};

use image::{DynamicImage, ImageFormat, ImageReader};

/// Longest side sent to the vision encoder (it downsamples further itself).
pub const MODEL_MAX_SIDE: u32 = 1536;
/// Longest side kept for OCR (small text needs pixels).
pub const OCR_MAX_SIDE: u32 = 4096;
/// Refuse to decode images above this many pixels (decompression bombs).
pub const MAX_PIXELS: u64 = 120_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// Format needs an OS codec that is not present.
    UnsupportedCodec,
    Corrupt,
    TooLarge,
    Empty,
}

impl MediaError {
    pub fn code(&self) -> &'static str {
        match self {
            MediaError::UnsupportedCodec => "unsupported_codec",
            MediaError::Corrupt => "corrupt",
            MediaError::TooLarge => "too_large",
            MediaError::Empty => "empty",
        }
    }
}

/// OS services the pipeline may use; implemented by the app with
/// `tamgabul_platform::media`, by tests with fakes.
pub trait Platform {
    /// Decode an image the portable decoder can't, to PNG.
    fn decode_to_png(&self, bytes: &[u8], max_side: u32) -> Option<Vec<u8>>;
    /// OCR an encoded image. `None` = OCR not available on this system.
    fn ocr(&self, png: &[u8]) -> Option<Result<String, String>>;
    fn ocr_available(&self) -> bool;
    /// PNG renders of the given 0-based PDF pages. `None` = unavailable.
    fn render_pdf_pages(
        &self,
        pdf: &[u8],
        pages: &[u32],
        width: u32,
    ) -> Option<Vec<(u32, Vec<u8>)>>;
    /// One representative frame of a video file (encoded image), from the
    /// OS thumbnailer. `None` = unavailable.
    fn video_frame(&self, _path: &std::path::Path) -> Option<Vec<u8>> {
        None
    }
}

/// No OS services (non-Windows today, or tests).
pub struct NoPlatform;

impl Platform for NoPlatform {
    fn decode_to_png(&self, _: &[u8], _: u32) -> Option<Vec<u8>> {
        None
    }
    fn ocr(&self, _: &[u8]) -> Option<Result<String, String>> {
        None
    }
    fn ocr_available(&self) -> bool {
        false
    }
    fn render_pdf_pages(&self, _: &[u8], _: &[u32], _: u32) -> Option<Vec<(u32, Vec<u8>)>> {
        None
    }
}

// ---------------------------------------------------------------- images

/// The PNG turned by a quarter (`clockwise`) or the other way. For OCR of
/// photos taken sideways (a passport page shot in portrait).
pub fn rotate_png(png: &[u8], clockwise: bool) -> Option<Vec<u8>> {
    let img = image::load_from_memory_with_format(png, ImageFormat::Png).ok()?;
    let r = if clockwise {
        img.rotate90()
    } else {
        img.rotate270()
    };
    png_bytes(&r).ok()
}

fn png_bytes(img: &DynamicImage) -> Result<Vec<u8>, MediaError> {
    let mut out = Cursor::new(Vec::new());
    // RGBA8/RGB8 PNG: what every consumer understands.
    let img = if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    };
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|_| MediaError::Corrupt)?;
    Ok(out.into_inner())
}

fn bounded(img: DynamicImage, max_side: u32) -> DynamicImage {
    if img.width().max(img.height()) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3)
    } else {
        img
    }
}

pub struct NormalizedImage {
    /// For the vision encoder.
    pub model_png: Vec<u8>,
    /// For OCR (higher resolution).
    pub ocr_png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn portable_decode(bytes: &[u8], ext: Option<&str>) -> Result<DynamicImage, MediaError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| MediaError::Corrupt)?;
    if reader.format().is_none() {
        // Formats without a magic number (TGA) go by extension.
        match ext.and_then(ImageFormat::from_extension) {
            Some(f) => reader.set_format(f),
            None => return Err(MediaError::UnsupportedCodec),
        }
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => MediaError::TooLarge,
        image::ImageError::Unsupported(_) => MediaError::UnsupportedCodec,
        _ => MediaError::Corrupt,
    })
}

/// Decode any supported image to bounded PNGs (model + OCR sizes).
pub fn normalize_image(
    bytes: &[u8],
    ext: Option<&str>,
    platform: &dyn Platform,
) -> Result<NormalizedImage, MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    let img = match portable_decode(bytes, ext) {
        Ok(i) => i,
        Err(MediaError::UnsupportedCodec) | Err(MediaError::Corrupt) => {
            // HEIC/AVIF/JPEG XR/RAW… through the OS codecs.
            let png = platform
                .decode_to_png(bytes, OCR_MAX_SIDE)
                .ok_or(MediaError::UnsupportedCodec)?;
            portable_decode(&png, Some("png"))?
        }
        Err(e) => return Err(e),
    };
    // Animated GIF/WebP: the first frame represents the file.
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(MediaError::Empty);
    }
    let ocr = bounded(img, OCR_MAX_SIDE);
    let model = bounded(ocr.clone(), MODEL_MAX_SIDE);
    Ok(NormalizedImage {
        model_png: png_bytes(&model)?,
        ocr_png: png_bytes(&ocr)?,
        width: w,
        height: h,
    })
}

// ---------------------------------------------------------------- audio

pub const AUDIO_RATE: u32 = 16_000;
/// Seconds per audio embedding (25 tokens/s → 750 tokens each).
pub const SEGMENT_SECS: u32 = 30;
/// Longest audio decoded per file; longer recordings are covered evenly.
pub const MAX_AUDIO_SECS: u32 = 3 * 60 * 60;
pub const MAX_SEGMENTS: usize = 40;

/// Decode to 16 kHz mono f32. `ext` helps the probe.
pub fn decode_audio(bytes: Vec<u8>, ext: Option<&str>) -> Result<Vec<f32>, MediaError> {
    decode_audio_source(Box::new(Cursor::new(bytes)), ext)
}

/// The audio track of a (possibly large) audio or video file, streamed
/// from disk — the file is never read whole. Opened read-only.
pub fn decode_audio_file(
    path: &std::path::Path,
    ext: Option<&str>,
) -> Result<Vec<f32>, MediaError> {
    let f = std::fs::File::open(path).map_err(|_| MediaError::Corrupt)?;
    decode_audio_source(Box::new(f), ext)
}

fn decode_audio_source(
    src: Box<dyn symphonia::core::io::MediaSource>,
    ext: Option<&str>,
) -> Result<Vec<f32>, MediaError> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SErr;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let mss = MediaSourceStream::new(src, Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ext {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| MediaError::UnsupportedCodec)?;
    let mut format = probed.format;
    // The audio track (video containers also list their video track).
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .or_else(|| {
            format
                .tracks()
                .iter()
                .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        })
        .ok_or(MediaError::Empty)?;
    let track_id = track.id;
    let src_rate = track.codec_params.sample_rate.unwrap_or(AUDIO_RATE).max(1);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| MediaError::UnsupportedCodec)?;

    let max_src = src_rate as u64 * MAX_AUDIO_SECS as u64;
    let mut mono: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SErr::IoError(_)) | Err(SErr::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SErr::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let sb =
            buf.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if sb.capacity() < decoded.capacity() * channels {
            *sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
        if mono.len() as u64 >= max_src {
            break;
        }
    }
    if mono.is_empty() {
        return Err(MediaError::Empty);
    }
    Ok(resample(&mono, src_rate, AUDIO_RATE))
}

/// Linear resampler; adequate for embedding input.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let n = (x.len() as f64 / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let j = p.floor() as usize;
            let f = (p - j as f64) as f32;
            let a = x[j.min(x.len() - 1)];
            let b = x[(j + 1).min(x.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

pub fn wav16(samples: &[f32]) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        pcm.extend(((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16).to_le_bytes());
    }
    let mut w = b"RIFF".to_vec();
    w.extend((36 + pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(AUDIO_RATE.to_le_bytes());
    w.extend((AUDIO_RATE * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend((pcm.len() as u32).to_le_bytes());
    w.extend(pcm);
    w
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub wav: Vec<u8>,
}

/// Fixed-length segments; silent ones are dropped. Recordings longer than
/// `MAX_SEGMENTS` segments are sampled evenly across their length.
pub fn audio_segments(samples: &[f32]) -> Vec<AudioSegment> {
    let seg = (SEGMENT_SECS * AUDIO_RATE) as usize;
    let total = samples.len().div_ceil(seg).max(1);
    let picks: Vec<usize> = if total <= MAX_SEGMENTS {
        (0..total).collect()
    } else {
        (0..MAX_SEGMENTS)
            .map(|i| i * total / MAX_SEGMENTS)
            .collect()
    };
    picks
        .into_iter()
        .filter_map(|i| {
            let a = i * seg;
            let b = (a + seg).min(samples.len());
            let s = samples.get(a..b)?;
            let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
            // Skip silence and fragments under a second.
            (rms > 0.003 && s.len() >= AUDIO_RATE as usize).then(|| AudioSegment {
                start_ms: a as u64 * 1000 / AUDIO_RATE as u64,
                end_ms: b as u64 * 1000 / AUDIO_RATE as u64,
                wav: wav16(s),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- documents

const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text of markup: block-ending tags become line breaks, the rest is
/// dropped; `<script>`/`<style>` content is skipped.
pub fn strip_markup(xml: &str, block_tags: &[&str]) -> String {
    let mut out = String::with_capacity(xml.len() / 2);
    let mut i = 0;
    let b = xml.as_bytes();
    let mut skip_until: Option<&str> = None;
    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = xml[i..].find('>') else { break };
            let tag = &xml[i + 1..i + end];
            let name = tag
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(stop) = skip_until {
                if tag.starts_with('/') && name == stop {
                    skip_until = None;
                }
            } else if !tag.starts_with('/')
                && (name == "script" || name == "style")
                && !tag.ends_with('/')
            {
                skip_until = Some(if name == "script" { "script" } else { "style" });
            } else if block_tags.iter().any(|t| *t == name)
                && (tag.starts_with('/')
                    || tag.ends_with('/')
                    || name.ends_with("br")
                    || name.ends_with("line-break")
                    || name == "w:cr")
            {
                // Line break at the end of a block (or a <br/>), not at both ends.
                out.push('\n');
            } else if name == "w:tab" || name == "text:tab" {
                out.push('\t');
            }
            i += end + 1;
            continue;
        }
        let next = xml[i..].find('<').map(|n| i + n).unwrap_or(b.len());
        if skip_until.is_none() {
            out.push_str(&decode_entities(&xml[i..next]));
        }
        i = next;
    }
    // Collapse runs of blank lines.
    let mut lines: Vec<&str> = Vec::new();
    for l in out.lines().map(str::trim_end) {
        if l.trim().is_empty() && lines.last().is_some_and(|p| p.trim().is_empty()) {
            continue;
        }
        lines.push(l);
    }
    lines.join("\n").trim().to_string()
}

pub const HTML_BLOCKS: &[&str] = &[
    "p",
    "br",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "td",
    "th",
    "pre",
    "blockquote",
    "title",
];
const OOXML_BLOCKS: &[&str] = &[
    "w:p",
    "w:br",
    "w:cr",
    "a:p",
    "a:br",
    "si",
    "row",
    "text:p",
    "text:h",
    "text:line-break",
    "table:table-row",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    Docx,
    Pptx,
    Xlsx,
    OpenDocument,
}

fn zip_entry(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let f = z.by_name(name).ok()?;
    if f.size() > MAX_XML_BYTES {
        return None;
    }
    let mut s = String::new();
    f.take(MAX_XML_BYTES).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Text of an Office Open XML or OpenDocument file.
pub fn office_text(bytes: &[u8], kind: OfficeKind) -> Result<String, MediaError> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| MediaError::Corrupt)?;
    let mut parts: Vec<String> = Vec::new();
    match kind {
        OfficeKind::Docx => {
            for n in [
                "word/document.xml",
                "word/footnotes.xml",
                "word/endnotes.xml",
            ] {
                if let Some(x) = zip_entry(&mut z, n) {
                    parts.push(x);
                }
            }
        }
        OfficeKind::Pptx | OfficeKind::Xlsx => {
            let prefix = if kind == OfficeKind::Pptx {
                "ppt/slides/slide"
            } else {
                "xl/worksheets/sheet"
            };
            let mut names: Vec<String> = z
                .file_names()
                .filter(|n| n.starts_with(prefix) && n.ends_with(".xml"))
                .map(str::to_owned)
                .collect();
            // slide2 before slide10.
            names.sort_by_key(|n| {
                n.trim_start_matches(prefix)
                    .trim_end_matches(".xml")
                    .parse::<u32>()
                    .unwrap_or(u32::MAX)
            });
            if kind == OfficeKind::Xlsx {
                if let Some(x) = zip_entry(&mut z, "xl/sharedStrings.xml") {
                    parts.push(x);
                }
            }
            for n in names.iter().take(500) {
                if let Some(x) = zip_entry(&mut z, n) {
                    parts.push(x);
                }
            }
        }
        OfficeKind::OpenDocument => {
            if let Some(x) = zip_entry(&mut z, "content.xml") {
                parts.push(x);
            }
        }
    }
    let text = parts
        .iter()
        .map(|p| strip_markup(p, OOXML_BLOCKS))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.trim().is_empty() {
        Err(MediaError::Empty)
    } else {
        Ok(text)
    }
}

/// Plain text of RTF: control words and groups dropped, `\par` → newline,
/// `\'xx` decoded as Windows-1254.
pub fn rtf_text(src: &str) -> String {
    let mut out = String::new();
    let mut chars = src.chars().peekable();
    let mut depth_skip: Vec<bool> = vec![false];
    while let Some(c) = chars.next() {
        let skipping = *depth_skip.last().unwrap_or(&false);
        match c {
            '{' => depth_skip.push(skipping),
            '}' => {
                depth_skip.pop();
                if depth_skip.is_empty() {
                    depth_skip.push(false);
                }
            }
            '\\' => {
                let mut word = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphabetic() {
                        word.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if word.is_empty() {
                    match chars.next() {
                        Some('\'') => {
                            let h: String = chars.by_ref().take(2).collect();
                            if let Ok(b) = u8::from_str_radix(&h, 16) {
                                if !skipping {
                                    out.push_str(
                                        &crate::index::text::decode(&[b]).unwrap_or_default(),
                                    );
                                }
                            }
                        }
                        Some('*') => {
                            if let Some(s) = depth_skip.last_mut() {
                                *s = true;
                            }
                        }
                        Some(x) if !skipping && "\\{}".contains(x) => out.push(x),
                        _ => {}
                    }
                    continue;
                }
                // optional numeric parameter and one space delimiter
                let mut param = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_digit() || (n == '-' && param.is_empty()) {
                        param.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                match word.as_str() {
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header"
                    | "footer" => {
                        if let Some(s) = depth_skip.last_mut() {
                            *s = true;
                        }
                    }
                    "par" | "line" | "row" if !skipping => out.push('\n'),
                    "tab" | "cell" if !skipping => out.push('\t'),
                    "u" if !skipping => {
                        if let Some(ch) = param
                            .parse::<i32>()
                            .ok()
                            .and_then(|v| char::from_u32(v.rem_euclid(65536) as u32))
                        {
                            out.push(ch);
                            // skip the ANSI fallback character
                            if chars
                                .peek()
                                .is_some_and(|c| *c != '\\' && *c != '{' && *c != '}')
                            {
                                chars.next();
                            }
                        }
                    }
                    _ => {}
                }
            }
            '\r' | '\n' => {}
            _ if !skipping => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        }));
        let mut c = Cursor::new(Vec::new());
        img.write_to(&mut c, ImageFormat::Png).unwrap();
        c.into_inner()
    }

    fn encode(img: &DynamicImage, f: ImageFormat) -> Vec<u8> {
        let mut c = Cursor::new(Vec::new());
        img.write_to(&mut c, f).unwrap();
        c.into_inner()
    }

    #[test]
    fn many_image_formats_normalize_to_bounded_png() {
        let base = DynamicImage::ImageRgb8(image::RgbImage::from_fn(40, 30, |x, y| {
            image::Rgb([x as u8 * 6, y as u8 * 8, 120])
        }));
        for f in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Tiff,
            ImageFormat::WebP,
            ImageFormat::Pnm,
            ImageFormat::Tga,
            ImageFormat::Qoi,
        ] {
            let bytes = encode(&base, f);
            let n = normalize_image(&bytes, Some(f.extensions_str()[0]), &NoPlatform)
                .unwrap_or_else(|e| panic!("{f:?}: {e:?}"));
            assert_eq!(&n.model_png[..4], b"\x89PNG", "{f:?}");
            assert_eq!((n.width, n.height), (40, 30), "{f:?}");
        }
        let ico = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([1, 2, 3, 255]),
        ));
        assert_eq!(
            normalize_image(&encode(&ico, ImageFormat::Ico), Some("ico"), &NoPlatform)
                .unwrap()
                .width,
            32
        );
        let big = normalize_image(&png(4000, 1000), None, &NoPlatform).unwrap();
        let m = image::load_from_memory(&big.model_png).unwrap();
        assert_eq!(m.width(), MODEL_MAX_SIDE);
        let o = image::load_from_memory(&big.ocr_png).unwrap();
        assert_eq!(o.width(), 4000);
    }

    struct FakeOs;
    impl Platform for FakeOs {
        fn decode_to_png(&self, _: &[u8], _: u32) -> Option<Vec<u8>> {
            Some(png(8, 8))
        }
        fn ocr(&self, _: &[u8]) -> Option<Result<String, String>> {
            None
        }
        fn ocr_available(&self) -> bool {
            false
        }
        fn render_pdf_pages(&self, _: &[u8], _: &[u32], _: u32) -> Option<Vec<(u32, Vec<u8>)>> {
            None
        }
    }

    #[test]
    fn os_codecs_are_the_fallback_for_heic_and_friends() {
        let heic_like = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic";
        assert_eq!(
            normalize_image(heic_like, Some("heic"), &NoPlatform).err(),
            Some(MediaError::UnsupportedCodec)
        );
        assert_eq!(
            normalize_image(heic_like, Some("heic"), &FakeOs)
                .unwrap()
                .width,
            8
        );
        assert_eq!(
            normalize_image(b"", None, &NoPlatform).err(),
            Some(MediaError::Empty)
        );
    }

    #[test]
    fn audio_decodes_resamples_and_segments() {
        // 75 s of 44.1 kHz stereo tone as WAV.
        let rate = 44_100u32;
        let n = rate as usize * 75;
        let mut pcm = Vec::with_capacity(n * 4);
        for i in 0..n {
            let s = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin() * 8000.0)
                as i16;
            pcm.extend(s.to_le_bytes());
            pcm.extend(s.to_le_bytes());
        }
        let mut w = b"RIFF".to_vec();
        w.extend((36 + pcm.len() as u32).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(rate.to_le_bytes());
        w.extend((rate * 4).to_le_bytes());
        w.extend(4u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend((pcm.len() as u32).to_le_bytes());
        w.extend(pcm);
        let mono = decode_audio(w, Some("wav")).unwrap();
        let secs = mono.len() as f32 / AUDIO_RATE as f32;
        assert!((secs - 75.0).abs() < 0.1, "{secs}");
        let segs = audio_segments(&mono);
        assert_eq!(segs.len(), 3);
        assert_eq!((segs[0].start_ms, segs[0].end_ms), (0, 30_000));
        assert_eq!(segs[2].end_ms, 75_000);
        assert_eq!(&segs[0].wav[..4], b"RIFF");
        assert!(decode_audio(b"not audio at all".to_vec(), Some("mp3")).is_err());
    }

    #[test]
    fn long_recordings_are_sampled_and_silence_dropped() {
        let quiet = vec![0.0f32; AUDIO_RATE as usize * 60];
        assert!(audio_segments(&quiet).is_empty());
        let loud: Vec<f32> = (0..AUDIO_RATE as usize * SEGMENT_SECS as usize * 100)
            .map(|i| ((i % 50) as f32 / 50.0) - 0.5)
            .collect();
        assert_eq!(audio_segments(&loud).len(), MAX_SEGMENTS);
    }

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut c = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut c);
            for (n, body) in files {
                z.start_file(*n, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut z, body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        c.into_inner()
    }

    #[test]
    fn office_documents() {
        let docx = zip_with(&[(
            "word/document.xml",
            r#"<w:document><w:body><w:p><w:r><w:t>Bütçe &amp; plan</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">ikinci </w:t></w:r><w:r><w:t>satır</w:t></w:r></w:p></w:body></w:document>"#,
        )]);
        assert_eq!(
            office_text(&docx, OfficeKind::Docx).unwrap(),
            "Bütçe & plan\nikinci satır"
        );
        let pptx = zip_with(&[
            (
                "ppt/slides/slide10.xml",
                "<p:sld><a:p><a:r><a:t>Ten</a:t></a:r></a:p></p:sld>",
            ),
            (
                "ppt/slides/slide2.xml",
                "<p:sld><a:p><a:r><a:t>Two</a:t></a:r></a:p></p:sld>",
            ),
        ]);
        assert_eq!(office_text(&pptx, OfficeKind::Pptx).unwrap(), "Two\n\nTen");
        let xlsx = zip_with(&[
            (
                "xl/sharedStrings.xml",
                "<sst><si><t>Gelir</t></si><si><t>Gider</t></si></sst>",
            ),
            (
                "xl/worksheets/sheet1.xml",
                "<worksheet><sheetData><row><c><v>42</v></c></row></sheetData></worksheet>",
            ),
        ]);
        let t = office_text(&xlsx, OfficeKind::Xlsx).unwrap();
        assert!(
            t.contains("Gelir") && t.contains("Gider") && t.contains("42"),
            "{t}"
        );
        let odt = zip_with(&[(
            "content.xml",
            "<office:text><text:p>Merhaba</text:p><text:p>dünya</text:p></office:text>",
        )]);
        assert_eq!(
            office_text(&odt, OfficeKind::OpenDocument).unwrap(),
            "Merhaba\ndünya"
        );
        assert_eq!(
            office_text(b"nope", OfficeKind::Docx).err(),
            Some(MediaError::Corrupt)
        );
    }

    #[test]
    fn html_and_rtf() {
        let html = "<html><head><title>Rapor</title><style>p{color:red}</style><script>var x=1;</script></head><body><h1>Başlık</h1><p>A &lt; B &#x131;</p></body></html>";
        assert_eq!(strip_markup(html, HTML_BLOCKS), "Rapor\nBaşlık\nA < B ı");
        let rtf = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0 Merhaba \'fe\'f0\par ikinci\u305?satir}";
        assert_eq!(rtf_text(rtf), "Merhaba şğ\nikinciısatir");
    }
}
