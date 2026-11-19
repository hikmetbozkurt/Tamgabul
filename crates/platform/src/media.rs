//! OS media services used by indexing (P1 OCR, P8 macOS/Linux):
//!
//! - **Windows:** `Windows.Media.Ocr`, WIC (`Windows.Graphics.Imaging`),
//!   `Windows.Data.Pdf`, and Shell video thumbnails.
//! - **macOS:** built-in Vision (`VNRecognizeTextRequest`), `PDFKit`, `/usr/bin/sips`
//!   (HEIC/HEIF/RAW decoding), and `/usr/bin/qlmanage` (video thumbnails),
//!   using `/usr/bin/osascript -l JavaScript` so no Xcode CLT prompt triggers.
//! - **Linux:** optional system CLI probes (`tesseract`, `pdfinfo`/`pdftoppm`,
//!   `ffmpeg`, `heif-convert`/`magick`); cleanly returns `MediaError::Unsupported`
//!   when tools are absent.
//!
//! All functions are blocking and safe to call from any worker thread.

#[cfg(any(unix, test))]
use std::ffi::OsString;
use std::fmt;
#[cfg(any(unix, test))]
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// Not available on this OS (or no OCR language installed).
    Unsupported,
    /// The OS could not decode/process this input.
    Failed(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Unsupported => write!(f, "unsupported on this system"),
            MediaError::Failed(m) => write!(f, "{m}"),
        }
    }
}

/// Text found in an image, one entry per line, in reading order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OcrText {
    pub lines: Vec<String>,
    /// BCP-47 tag of the recognizer used, e.g. "tr" or "en-US".
    pub language: String,
}

impl OcrText {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

#[cfg(windows)]
// WinRT `Param` accepts both owned and borrowed interfaces; borrowing is
// the documented windows-rs style.
#[allow(clippy::needless_borrows_for_generic_args)]
mod imp {
    use super::{MediaError, OcrText};
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapInterpolationMode, BitmapPixelFormat,
        BitmapTransform, ColorManagementMode, ExifOrientationMode, SoftwareBitmap,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    fn fail(e: windows::core::Error) -> MediaError {
        MediaError::Failed(format!("0x{:08X}", e.code().0))
    }

    fn init() {
        // Worker threads join the multithreaded apartment. A thread that is
        // already initialized answers S_FALSE/RPC_E_CHANGED_MODE: fine.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    }

    fn stream_from(bytes: &[u8]) -> windows::core::Result<InMemoryRandomAccessStream> {
        let s = InMemoryRandomAccessStream::new()?;
        let w = DataWriter::CreateDataWriter(&s)?;
        w.WriteBytes(bytes)?;
        w.StoreAsync()?.get()?;
        w.FlushAsync()?.get()?;
        w.DetachStream()?;
        s.Seek(0)?;
        Ok(s)
    }

    fn bytes_from(s: &InMemoryRandomAccessStream) -> windows::core::Result<Vec<u8>> {
        let size = s.Size()? as u32;
        let input = s.GetInputStreamAt(0)?;
        let r = DataReader::CreateDataReader(&input)?;
        r.LoadAsync(size)?.get()?;
        let mut v = vec![0u8; size as usize];
        r.ReadBytes(&mut v)?;
        Ok(v)
    }

    /// Decode any WIC-supported image, upright (EXIF), scaled so the longer
    /// side is at most `max_side`.
    fn decode(bytes: &[u8], max_side: u32) -> windows::core::Result<SoftwareBitmap> {
        let s = stream_from(bytes)?;
        let dec = BitmapDecoder::CreateAsync(&s)?.get()?;
        let (w, h) = (dec.PixelWidth()?, dec.PixelHeight()?);
        let t = BitmapTransform::new()?;
        let longest = w.max(h).max(1);
        if longest > max_side {
            let scale = max_side as f64 / longest as f64;
            t.SetScaledWidth(((w as f64 * scale).round() as u32).max(1))?;
            t.SetScaledHeight(((h as f64 * scale).round() as u32).max(1))?;
            t.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
        }
        dec.GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &t,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()
    }

    fn encode_png(bmp: &SoftwareBitmap) -> windows::core::Result<Vec<u8>> {
        let out = InMemoryRandomAccessStream::new()?;
        let enc = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &out)?.get()?;
        enc.SetSoftwareBitmap(bmp)?;
        enc.FlushAsync()?.get()?;
        bytes_from(&out)
    }

    pub fn decode_to_png(bytes: &[u8], max_side: u32) -> Result<Vec<u8>, MediaError> {
        init();
        let bmp = decode(bytes, max_side).map_err(fail)?;
        encode_png(&bmp).map_err(fail)
    }

    pub fn ocr(bytes: &[u8]) -> Result<OcrText, MediaError> {
        init();
        let engine =
            OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| MediaError::Unsupported)?;
        let language = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string())
            .unwrap_or_default();
        let max = OcrEngine::MaxImageDimension().unwrap_or(4096).max(256);
        let bmp = decode(bytes, max).map_err(fail)?;
        let result = engine
            .RecognizeAsync(&bmp)
            .map_err(fail)?
            .get()
            .map_err(fail)?;
        let lines = result.Lines().map_err(fail)?;
        let n = lines.Size().map_err(fail)?;
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let t = lines
                .GetAt(i)
                .and_then(|l| l.Text())
                .map_err(fail)?
                .to_string();
            let t = t.trim().to_string();
            if !t.is_empty() {
                out.push(t);
            }
        }
        Ok(OcrText {
            lines: out,
            language,
        })
    }

    pub fn video_frame(path: &std::path::Path, size: u32) -> Result<Vec<u8>, MediaError> {
        use windows::core::HSTRING;
        use windows::Storage::FileProperties::{ThumbnailMode, ThumbnailOptions, ThumbnailType};
        use windows::Storage::StorageFile;
        init();
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
            .map_err(fail)?
            .get()
            .map_err(fail)?;
        let thumb = file
            .GetThumbnailAsync(
                ThumbnailMode::SingleItem,
                size,
                ThumbnailOptions::ResizeThumbnail,
            )
            .map_err(fail)?
            .get()
            .map_err(fail)?;
        // No codec for this video: Windows hands back the file-type icon.
        if thumb.Type().map_err(fail)? != ThumbnailType::Image {
            return Err(MediaError::Unsupported);
        }
        let n = thumb.Size().map_err(fail)? as u32;
        let input = thumb.GetInputStreamAt(0).map_err(fail)?;
        let r = DataReader::CreateDataReader(&input).map_err(fail)?;
        r.LoadAsync(n).map_err(fail)?.get().map_err(fail)?;
        let mut raw = vec![0u8; n as usize];
        r.ReadBytes(&mut raw).map_err(fail)?;
        // Re-encode whatever the shell returned (BMP/PNG/JPEG) as PNG.
        let bmp = decode(&raw, size).map_err(fail)?;
        encode_png(&bmp).map_err(fail)
    }

    pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, MediaError> {
        init();
        let s = stream_from(bytes).map_err(fail)?;
        let doc = PdfDocument::LoadFromStreamAsync(&s)
            .map_err(fail)?
            .get()
            .map_err(fail)?;
        doc.PageCount().map_err(fail)
    }

    pub fn pdf_render_pages(
        bytes: &[u8],
        pages: &[u32],
        width: u32,
        f: &mut dyn FnMut(u32, Vec<u8>),
    ) -> Result<(), MediaError> {
        init();
        let s = stream_from(bytes).map_err(fail)?;
        let doc = PdfDocument::LoadFromStreamAsync(&s)
            .map_err(fail)?
            .get()
            .map_err(fail)?;
        let count = doc.PageCount().map_err(fail)?;
        let png = BitmapEncoder::PngEncoderId().map_err(fail)?;
        for &i in pages.iter().filter(|&&i| i < count) {
            let page = doc.GetPage(i).map_err(fail)?;
            let out = InMemoryRandomAccessStream::new().map_err(fail)?;
            let opts = PdfPageRenderOptions::new().map_err(fail)?;
            opts.SetDestinationWidth(width).map_err(fail)?;
            opts.SetBitmapEncoderId(png).map_err(fail)?;
            page.RenderWithOptionsToStreamAsync(&out, &opts)
                .map_err(fail)?
                .get()
                .map_err(fail)?;
            f(i, bytes_from(&out).map_err(fail)?);
        }
        Ok(())
    }
}

#[cfg(any(unix, test))]
fn command_exists(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
}

#[cfg(any(unix, test))]
fn parse_ocr_lines(stdout: &str, language: &str) -> OcrText {
    let lines = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    OcrText {
        lines,
        language: language.to_owned(),
    }
}

#[cfg(any(unix, test))]
fn parse_pdfinfo_pages(stdout: &str) -> Option<u32> {
    stdout.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("pages")
            .then(|| v.trim().parse::<u32>().ok())
            .flatten()
    })
}

#[cfg(any(target_os = "macos", test))]
fn build_sips_args(input: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-s".into(),
        "format".into(),
        "png".into(),
        input.as_os_str().to_os_string(),
        "--out".into(),
        output.as_os_str().to_os_string(),
    ]
}

#[cfg(any(target_os = "macos", test))]
fn build_qlmanage_args(path: &Path, size: u32, out_dir: &Path) -> Vec<OsString> {
    vec![
        "-t".into(),
        "-s".into(),
        size.to_string().into(),
        "-o".into(),
        out_dir.as_os_str().to_os_string(),
        path.as_os_str().to_os_string(),
    ]
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn build_pdftoppm_args(
    pdf_path: &Path,
    page_zero_based: u32,
    width: u32,
    out_prefix: &Path,
) -> Vec<OsString> {
    let page_one = (page_zero_based + 1).to_string();
    vec![
        "-png".into(),
        "-singlefile".into(),
        "-scale-to-x".into(),
        width.to_string().into(),
        "-scale-to-y".into(),
        "-1".into(),
        "-f".into(),
        page_one.clone().into(),
        "-l".into(),
        page_one.into(),
        pdf_path.as_os_str().to_os_string(),
        out_prefix.as_os_str().to_os_string(),
    ]
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn build_ffmpeg_frame_args(path: &Path, size: u32, seek_sec: u32) -> Vec<OsString> {
    vec![
        "-v".into(),
        "error".into(),
        "-ss".into(),
        seek_sec.to_string().into(),
        "-i".into(),
        path.as_os_str().to_os_string(),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min(iw,{size})':'min(ih,{size})':force_original_aspect_ratio=decrease")
            .into(),
        "-f".into(),
        "image2pipe".into(),
        "-vcodec".into(),
        "png".into(),
        "-".into(),
    ]
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{build_qlmanage_args, build_sips_args, parse_ocr_lines, MediaError, OcrText};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::{Command, Stdio};

    const OSASCRIPT: &str = "/usr/bin/osascript";
    const SIPS: &str = "/usr/bin/sips";
    const QLMANAGE: &str = "/usr/bin/qlmanage";

    const JXA_OCR: &str = r#"ObjC.import('Vision');
ObjC.import('AppKit');
ObjC.import('Foundation');
function run(argv) {
  var img = $.NSImage.alloc.initWithContentsOfFile(argv[0]);
  if (!img) throw new Error('decode_failed');
  var data = img.TIFFRepresentation;
  if (!data) throw new Error('tiff_failed');
  var handler = $.VNImageRequestHandler.alloc.initWithDataOptions(data, $.NSDictionary.dictionary);
  var req = $.VNRecognizeTextRequest.alloc.init;
  req.recognitionLevel = $.VNRequestTextRecognitionLevelAccurate;
  req.usesLanguageCorrection = true;
  var ok = handler.performRequestsError([req], null);
  if (!ok) throw new Error('vision_failed');
  var res = req.results;
  var out = [];
  for (var i = 0; i < res.count; i++) {
    var top = res.objectAtIndex(i).topCandidates(1);
    if (top.count > 0) out.push(ObjC.unwrap(top.objectAtIndex(0).string));
  }
  return out.join('\n');
}"#;

    const JXA_PDF_COUNT: &str = r#"ObjC.import('PDFKit');
ObjC.import('Foundation');
function run(argv) {
  var doc = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath(argv[0]));
  if (!doc) throw new Error('pdf_open_failed');
  return String(doc.pageCount);
}"#;

    const JXA_PDF_RENDER: &str = r#"ObjC.import('PDFKit');
ObjC.import('AppKit');
ObjC.import('Foundation');
function run(argv) {
  var pdfPath = argv[0];
  var pageIdx = parseInt(argv[1], 10);
  var width = parseFloat(argv[2]);
  var outPath = argv[3];
  var doc = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath(pdfPath));
  if (!doc) throw new Error('pdf_open_failed');
  var page = doc.pageAtIndex(pageIdx);
  if (!page) throw new Error('page_out_of_range');
  var img = page.thumbnailOfSizeForBox($.NSMakeSize(width, width), $.kPDFDisplayBoxMediaBox);
  if (!img) throw new Error('render_failed');
  var tiff = img.TIFFRepresentation;
  var rep = $.NSBitmapImageRep.imageRepWithData(tiff);
  if (!rep) throw new Error('rep_failed');
  var png = rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $.NSDictionary.dictionary);
  if (!png || !png.writeToFileAtomically(outPath, true)) throw new Error('write_failed');
  return 'ok';
}"#;

    fn run_jxa(
        script_dir: &Path,
        name: &str,
        script: &str,
        args: &[&OsStr],
    ) -> Result<String, MediaError> {
        if !Path::new(OSASCRIPT).is_file() {
            return Err(MediaError::Unsupported);
        }
        let script_path = script_dir.join(name);
        std::fs::write(&script_path, script).map_err(|e| MediaError::Failed(e.to_string()))?;
        let mut cmd = Command::new(OSASCRIPT);
        cmd.args(["-l", "JavaScript"])
            .arg(&script_path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd
            .output()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(MediaError::Failed("osascript failed".into()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn ocr_supported() -> bool {
        Path::new(OSASCRIPT).is_file()
    }

    pub fn decode_to_png(bytes: &[u8], _max_side: u32) -> Result<Vec<u8>, MediaError> {
        if !Path::new(SIPS).is_file() {
            return Err(MediaError::Unsupported);
        }
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let input = dir.path().join("input.img");
        let output = dir.path().join("output.png");
        std::fs::write(&input, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        let out = Command::new(SIPS)
            .args(build_sips_args(&input, &output))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if !out.status.success() || !output.is_file() {
            return Err(MediaError::Failed("sips decode failed".into()));
        }
        std::fs::read(&output).map_err(|e| MediaError::Failed(e.to_string()))
    }

    pub fn ocr(bytes: &[u8]) -> Result<OcrText, MediaError> {
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let img = dir.path().join("ocr.img");
        std::fs::write(&img, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        let text = run_jxa(dir.path(), "ocr.js", JXA_OCR, &[img.as_os_str()])?;
        Ok(parse_ocr_lines(&text, "macos-vision"))
    }

    pub fn video_frame(path: &Path, size: u32) -> Result<Vec<u8>, MediaError> {
        if !Path::new(QLMANAGE).is_file() {
            return Err(MediaError::Unsupported);
        }
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let out = Command::new(QLMANAGE)
            .args(build_qlmanage_args(path, size, dir.path()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(MediaError::Failed("qlmanage failed".into()));
        }
        let rd = std::fs::read_dir(dir.path()).map_err(|e| MediaError::Failed(e.to_string()))?;
        let thumb = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| {
                p.is_file()
                    && p.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            })
            .ok_or(MediaError::Unsupported)?;
        std::fs::read(thumb).map_err(|e| MediaError::Failed(e.to_string()))
    }

    pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, MediaError> {
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let pdf = dir.path().join("doc.pdf");
        std::fs::write(&pdf, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        let out = run_jxa(dir.path(), "count.js", JXA_PDF_COUNT, &[pdf.as_os_str()])?;
        out.trim()
            .parse::<u32>()
            .map_err(|_| MediaError::Failed("invalid page count".into()))
    }

    pub fn pdf_render_pages(
        bytes: &[u8],
        pages: &[u32],
        width: u32,
        f: &mut dyn FnMut(u32, Vec<u8>),
    ) -> Result<(), MediaError> {
        if pages.is_empty() {
            return Ok(());
        }
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let pdf = dir.path().join("doc.pdf");
        std::fs::write(&pdf, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        let count = run_jxa(dir.path(), "count.js", JXA_PDF_COUNT, &[pdf.as_os_str()])?
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        for &i in pages.iter().filter(|&&i| i < count) {
            let out_png = dir.path().join(format!("page_{i}.png"));
            let idx_str = i.to_string();
            let width_str = width.to_string();
            run_jxa(
                dir.path(),
                "render.js",
                JXA_PDF_RENDER,
                &[
                    pdf.as_os_str(),
                    OsStr::new(&idx_str),
                    OsStr::new(&width_str),
                    out_png.as_os_str(),
                ],
            )?;
            let png = std::fs::read(&out_png).map_err(|e| MediaError::Failed(e.to_string()))?;
            f(i, png);
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{
        build_ffmpeg_frame_args, build_pdftoppm_args, command_exists, parse_ocr_lines,
        parse_pdfinfo_pages, MediaError, OcrText,
    };
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    static HAS_TESSERACT: OnceLock<bool> = OnceLock::new();

    pub fn ocr_supported() -> bool {
        *HAS_TESSERACT.get_or_init(|| command_exists("tesseract"))
    }

    pub fn decode_to_png(bytes: &[u8], _max_side: u32) -> Result<Vec<u8>, MediaError> {
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let input = dir.path().join("input.img");
        let output = dir.path().join("output.png");
        std::fs::write(&input, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        if command_exists("heif-convert") {
            let status = Command::new("heif-convert")
                .args([input.as_os_str(), output.as_os_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| MediaError::Failed(e.to_string()))?;
            if status.success() && output.is_file() {
                return std::fs::read(&output).map_err(|e| MediaError::Failed(e.to_string()));
            }
        }
        if command_exists("magick") {
            let status = Command::new("magick")
                .args([input.as_os_str(), output.as_os_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| MediaError::Failed(e.to_string()))?;
            if status.success() && output.is_file() {
                return std::fs::read(&output).map_err(|e| MediaError::Failed(e.to_string()));
            }
        }
        Err(MediaError::Unsupported)
    }

    pub fn ocr(bytes: &[u8]) -> Result<OcrText, MediaError> {
        if !ocr_supported() {
            return Err(MediaError::Unsupported);
        }
        let mut child = Command::new("tesseract")
            .args(["stdin", "stdout", "--psm", "3"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if let Some(mut sin) = child.stdin.take() {
            let _ = sin.write_all(bytes);
        }
        let out = child
            .wait_with_output()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(MediaError::Failed("tesseract failed".into()));
        }
        Ok(parse_ocr_lines(
            &String::from_utf8_lossy(&out.stdout),
            "tesseract",
        ))
    }

    pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, MediaError> {
        if !command_exists("pdfinfo") {
            return Err(MediaError::Unsupported);
        }
        let mut child = Command::new("pdfinfo")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if let Some(mut sin) = child.stdin.take() {
            let _ = sin.write_all(bytes);
        }
        let out = child
            .wait_with_output()
            .map_err(|e| MediaError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(MediaError::Failed("pdfinfo failed".into()));
        }
        parse_pdfinfo_pages(&String::from_utf8_lossy(&out.stdout))
            .ok_or_else(|| MediaError::Failed("pdf page count missing".into()))
    }

    pub fn video_frame(path: &Path, size: u32) -> Result<Vec<u8>, MediaError> {
        if !command_exists("ffmpeg") {
            return Err(MediaError::Unsupported);
        }
        for seek in [1u32, 0u32] {
            let out = Command::new("ffmpeg")
                .args(build_ffmpeg_frame_args(path, size, seek))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .map_err(|e| MediaError::Failed(e.to_string()))?;
            if out.status.success() && out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Ok(out.stdout);
            }
        }
        Err(MediaError::Failed("ffmpeg frame extraction failed".into()))
    }

    pub fn pdf_render_pages(
        bytes: &[u8],
        pages: &[u32],
        width: u32,
        f: &mut dyn FnMut(u32, Vec<u8>),
    ) -> Result<(), MediaError> {
        if !command_exists("pdftoppm") {
            return Err(MediaError::Unsupported);
        }
        if pages.is_empty() {
            return Ok(());
        }
        let count = pdf_page_count(bytes)?;
        let dir = tempfile::tempdir().map_err(|e| MediaError::Failed(e.to_string()))?;
        let pdf = dir.path().join("doc.pdf");
        std::fs::write(&pdf, bytes).map_err(|e| MediaError::Failed(e.to_string()))?;
        for &i in pages.iter().filter(|&&i| i < count) {
            let prefix = dir.path().join(format!("page_{i}"));
            let out = Command::new("pdftoppm")
                .args(build_pdftoppm_args(&pdf, i, width, &prefix))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|e| MediaError::Failed(e.to_string()))?;
            let rendered = dir.path().join(format!("page_{i}.png"));
            if !out.status.success() || !rendered.is_file() {
                continue;
            }
            if let Ok(png) = std::fs::read(&rendered) {
                f(i, png);
            }
        }
        Ok(())
    }
}

/// Decode an image the portable decoder can't read; result is PNG.
pub fn decode_to_png(bytes: &[u8], max_side: u32) -> Result<Vec<u8>, MediaError> {
    imp::decode_to_png(bytes, max_side)
}

/// Recognize text in an encoded image (PNG/JPEG/…).
pub fn ocr(bytes: &[u8]) -> Result<OcrText, MediaError> {
    imp::ocr(bytes)
}

pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, MediaError> {
    imp::pdf_page_count(bytes)
}

/// Render the given 0-based pages to PNG at `width` pixels.
pub fn pdf_render_pages(
    bytes: &[u8],
    pages: &[u32],
    width: u32,
    f: &mut dyn FnMut(u32, Vec<u8>),
) -> Result<(), MediaError> {
    imp::pdf_render_pages(bytes, pages, width, f)
}

/// One frame of a video file as PNG (longest side ≤ `size`).
pub fn video_frame(path: &std::path::Path, size: u32) -> Result<Vec<u8>, MediaError> {
    imp::video_frame(path, size)
}

pub fn ocr_supported() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        imp::ocr_supported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn native_media_arg_builders_and_parsers_work_cross_platform() {
        assert_eq!(parse_pdfinfo_pages("Title: Foo\nPages:    17\n"), Some(17));
        assert_eq!(parse_pdfinfo_pages("Encrypted: yes\n"), None);

        let ocr_out = parse_ocr_lines("  Line one \n\n  Line two  \n", "macos-vision");
        assert_eq!(ocr_out.lines, vec!["Line one", "Line two"]);
        assert_eq!(ocr_out.language, "macos-vision");

        assert_eq!(
            strs(&build_sips_args(
                Path::new("/a/in.heic"),
                Path::new("/b/out.png")
            )),
            vec!["-s", "format", "png", "/a/in.heic", "--out", "/b/out.png"]
        );
        assert_eq!(
            strs(&build_qlmanage_args(
                Path::new("/a/vid.mp4"),
                1536,
                Path::new("/tmp/out")
            )),
            vec!["-t", "-s", "1536", "-o", "/tmp/out", "/a/vid.mp4"]
        );
        assert_eq!(
            strs(&build_pdftoppm_args(
                Path::new("/a/doc.pdf"),
                0,
                1700,
                Path::new("/tmp/page_0")
            )),
            vec![
                "-png",
                "-singlefile",
                "-scale-to-x",
                "1700",
                "-scale-to-y",
                "-1",
                "-f",
                "1",
                "-l",
                "1",
                "/a/doc.pdf",
                "/tmp/page_0"
            ]
        );
        let ff = strs(&build_ffmpeg_frame_args(Path::new("/a/vid.mp4"), 1536, 1));
        assert!(ff.windows(2).any(|w| w == ["-ss", "1"]));
        assert!(ff.windows(2).any(|w| w == ["-vcodec", "png"]));
        assert!(!command_exists(
            "tamgabul_definitely_nonexistent_tool_12345"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_invalid_media_returns_error_or_unsupported() {
        assert!(matches!(
            ocr(b"not-an-image"),
            Err(MediaError::Unsupported) | Err(MediaError::Failed(_))
        ));
        assert!(matches!(
            decode_to_png(b"not-an-image", 100),
            Err(MediaError::Unsupported) | Err(MediaError::Failed(_))
        ));
    }
}
