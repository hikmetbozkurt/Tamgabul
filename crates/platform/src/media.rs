//! OS media services used by indexing (P1 OCR):
//!
//! - **OCR** of an image (`Windows.Media.Ocr`, uses the OCR languages that
//!   come with Windows for the user's profile languages — nothing is
//!   downloaded by Tamgabul).
//! - **Image decoding** of formats the portable decoder lacks (HEIC/HEIF,
//!   AVIF, JPEG XR, camera RAW…) through the Windows Imaging Component, which
//!   uses whatever codecs Windows has installed.
//! - **PDF page rendering** (`Windows.Data.Pdf`) so scanned pages can be read
//!   with OCR.
//! - **Video frame**: the shell thumbnail of a video file (one frame, made
//!   by the codecs Windows has; generic icons are refused).
//!
//! Other platforms answer `Unsupported` for now (macOS Vision/PDFKit later).
//! All functions are blocking and safe to call from any worker thread.

use std::fmt;

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

#[cfg(not(windows))]
mod imp {
    use super::{MediaError, OcrText};

    pub fn decode_to_png(_bytes: &[u8], _max_side: u32) -> Result<Vec<u8>, MediaError> {
        Err(MediaError::Unsupported)
    }
    pub fn ocr(_bytes: &[u8]) -> Result<OcrText, MediaError> {
        Err(MediaError::Unsupported)
    }
    pub fn pdf_page_count(_bytes: &[u8]) -> Result<u32, MediaError> {
        Err(MediaError::Unsupported)
    }
    pub fn video_frame(_path: &std::path::Path, _size: u32) -> Result<Vec<u8>, MediaError> {
        Err(MediaError::Unsupported)
    }
    pub fn pdf_render_pages(
        _bytes: &[u8],
        _pages: &[u32],
        _width: u32,
        _f: &mut dyn FnMut(u32, Vec<u8>),
    ) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
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
    cfg!(windows)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn non_windows_reports_unsupported() {
        assert_eq!(ocr(b"x"), Err(MediaError::Unsupported));
        assert_eq!(decode_to_png(b"x", 10), Err(MediaError::Unsupported));
        assert!(!ocr_supported());
    }
}
