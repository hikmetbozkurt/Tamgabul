//! PDF text extraction, isolated: a malformed or hostile PDF can make a
//! parser panic or spin, so extraction runs on its own thread with a time
//! limit and panics are caught (docs/07 "per-file parse timeout").

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// Encrypted, corrupt or unsupported.
    Unreadable,
    /// No text layer (scanned pages); OCR arrives later.
    NoText,
    Timeout,
}

impl PdfError {
    pub fn code(&self) -> &'static str {
        match self {
            PdfError::Unreadable => "pdf_unreadable",
            PdfError::NoText => "pdf_no_text",
            PdfError::Timeout => "pdf_timeout",
        }
    }
}

pub const PDF_TIMEOUT: Duration = Duration::from_secs(30);

/// Text per page (index 0 = page 1). Pages without text are empty strings.
pub fn pdf_pages(bytes: Vec<u8>, timeout: Duration) -> Result<Vec<String>, PdfError> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("tamgabul-pdf".into())
        .spawn(move || {
            let r =
                std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes));
            let _ = tx.send(match r {
                Ok(Ok(pages)) => Ok(pages),
                _ => Err(PdfError::Unreadable),
            });
        })
        .map_err(|_| PdfError::Unreadable)?;
    // On timeout the thread is left to finish on its own; its result is dropped.
    let pages = rx.recv_timeout(timeout).map_err(|_| PdfError::Timeout)??;
    let pages: Vec<String> = pages
        .into_iter()
        .map(|p| p.replace('\u{0}', "").trim().to_string())
        .collect();
    if pages
        .iter()
        .all(|p| p.chars().filter(|c| c.is_alphanumeric()).count() < 3)
    {
        return Err(PdfError::NoText);
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smallest valid one-page PDF with "Hello Tamgabul" in Helvetica.
    pub fn tiny_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn extracts_text_by_page() {
        let pages = pdf_pages(tiny_pdf("Hello Tamgabul"), PDF_TIMEOUT).unwrap();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].contains("Hello Tamgabul"), "{pages:?}");
    }

    #[test]
    fn garbage_is_unreadable_not_a_panic() {
        assert_eq!(
            pdf_pages(b"%PDF-1.4 garbage".to_vec(), PDF_TIMEOUT),
            Err(PdfError::Unreadable)
        );
        assert_eq!(
            pdf_pages(Vec::new(), PDF_TIMEOUT),
            Err(PdfError::Unreadable)
        );
    }

    #[test]
    fn empty_text_layer_is_no_text() {
        assert_eq!(pdf_pages(tiny_pdf(""), PDF_TIMEOUT), Err(PdfError::NoText));
    }
}
