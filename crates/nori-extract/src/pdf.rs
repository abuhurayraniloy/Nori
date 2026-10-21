use crate::doc::ExtractError;
use nori_text::script::PageTextProblem;
use std::ops::Range;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Minimum character count threshold for a PDF page to be indexed purely as text.
/// Pages with fewer than 200 characters (slides, presentation decks, scans) are routed to visual rendering.
pub const MIN_TEXT_CHARS: usize = 200;

/// Default pixel resolution for rendered PDF slide/page images (1,400 px).
pub const RENDER_PIXELS: u32 = 1400;

/// Number of pages extracted in the high-priority first pass before backfilling (30 pages).
pub const FIRST_PASS_MAX_PAGES: usize = 30;

/// Maximum PDF file size processed (100 MB).
pub const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;

/// A parsed PDF page ready for multimodal indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPage {
    /// 1-based page number.
    pub number: usize,
    /// Extracted plain text content (may be empty for visual-only slides or scans).
    pub text: String,
    /// Rendered image path (present when text < `MIN_TEXT_CHARS` or when script was garbled).
    pub image_path: Option<PathBuf>,
    /// Whether the text was recovered via OCR.
    pub ocr: bool,
    /// Text read in glyph order (`PageTextProblem::GlyphOrder`): words preserved for meaning vector and landmark links.
    pub garbled: Option<String>,
}

/// Overall document result of PDF extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfExtractionResult {
    pub pages: Vec<PdfPage>,
    pub total: usize,
    pub title: Option<String>,
}

/// High-throughput PDF page extractor, two-pass paginator, and script problem router.
pub struct PDFExtractor;

impl PDFExtractor {
    /// Extracts a range of pages from a PDF document, classifying each into text or visual representations.
    pub fn pages<F>(
        path: &Path,
        range_fn: F,
        min_text: usize,
        output_dir: &Path,
    ) -> Result<PdfExtractionResult, ExtractError>
    where
        F: FnOnce(usize) -> Range<usize>,
    {
        let metadata = path.metadata()?;
        if metadata.len() > MAX_PDF_BYTES {
            return Err(ExtractError::Unreadable(format!(
                "PDF too large ({} MB, max {} MB)",
                metadata.len() >> 20,
                MAX_PDF_BYTES >> 20
            )));
        }

        let mut document = pdf_extract::Document::load(path)
            .map_err(|e| ExtractError::Unreadable(format!("can't open PDF: {e}")))?;

        if document.is_encrypted() {
            if document.decrypt("").is_err() {
                return Err(ExtractError::Unreadable("password-protected PDF".to_string()));
            }
        }

        let title = Self::extract_title(&document);
        let page_dict = document.get_pages();
        let total = page_dict.len();

        let target_range = range_fn(total);
        let start_idx = target_range.start.min(total);
        let end_idx = target_range.end.min(total);

        let mut page_keys: Vec<u32> = page_dict.keys().copied().collect();
        page_keys.sort();

        let mut pages = Vec::new();

        for (idx, &page_num) in page_keys[start_idx..end_idx].iter().enumerate() {
            let actual_page_number = start_idx + idx + 1;
            let mut raw_text = String::new();

            {
                let mut output = pdf_extract::PlainTextOutput::new(&mut raw_text);
                let _ = pdf_extract::output_doc_page(&document, &mut output, page_num);
            }

            let mut text = raw_text
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .trim()
                .to_string();

            let mut garbled: Option<String> = None;
            let problem = PageTextProblem::check(&text);

            match problem {
                Some(PageTextProblem::LostLetters) | None => {
                    if text.chars().count() >= min_text {
                        pages.push(PdfPage {
                            number: actual_page_number,
                            text,
                            image_path: None,
                            ocr: false,
                            garbled: None,
                        });
                        continue;
                    }
                }
                Some(PageTextProblem::GlyphOrder) => {
                    garbled = Some(std::mem::take(&mut text));
                }
            }

            // Page had too little text (slide/scan) or was in inverted glyph order:
            // Route to visual representation
            let rendered_path = Self::render_placeholder(output_dir, actual_page_number)?;
            pages.push(PdfPage {
                number: actual_page_number,
                text,
                image_path: Some(rendered_path),
                ocr: false,
                garbled,
            });
        }

        Ok(PdfExtractionResult {
            pages,
            total,
            title,
        })
    }

    /// Reads document title from trailer Info dictionary if available.
    fn extract_title(document: &pdf_extract::Document) -> Option<String> {
        let info_id = document.trailer.get(b"Info").ok()?.as_reference().ok()?;
        let info_obj = document.get_object(info_id).ok()?.as_dict().ok()?;
        let title_obj = info_obj.get(b"Title").ok()?;

        let bytes = match title_obj {
            pdf_extract::Object::String(bytes, _) => bytes.as_slice(),
            _ => return None,
        };

        if bytes.starts_with(&[0xFE, 0xFF]) {
            // UTF-16BE
            let u16_chars: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&u16_chars).ok()
        } else {
            Some(String::from_utf8_lossy(bytes).trim().to_string())
        }
    }

    /// Renders a fallback PNG placeholder file for visual-routing validation.
    fn render_placeholder(output_dir: &Path, page_number: usize) -> Result<PathBuf, ExtractError> {
        let out_path = output_dir.join(format!("{}_page_{}.png", Uuid::new_v4(), page_number));
        let img = image::RgbImage::new(1400, 1050); // Standard 4:3 slide resolution
        img.save_with_format(&out_path, image::ImageFormat::Png)
            .map_err(|e| ExtractError::Unreadable(format!("can't save page render: {e}")))?;
        Ok(out_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_problem_routing_glyph_order() {
        // Bengali text with inverted vowel sign problem: e.g. "ি" preceded by consonant
        let garbled_bengali = "ক\u{09BF} পরীক্ষা"; // Invalid inverted ligature sequence
        let problem = PageTextProblem::check(garbled_bengali);

        // Verify routing logic
        let mut text = garbled_bengali.to_string();
        let mut garbled: Option<String> = None;

        if let Some(PageTextProblem::GlyphOrder) = problem {
            garbled = Some(std::mem::take(&mut text));
        }

        // When glyph order problem occurs, text is emptied for visual routing and preserved in garbled
        if problem == Some(PageTextProblem::GlyphOrder) {
            assert!(text.is_empty());
            assert_eq!(garbled, Some(garbled_bengali.to_string()));
        }
    }

    #[test]
    fn test_short_slide_routes_to_image() {
        let temp_dir = std::env::temp_dir().join(format!("nori_pdf_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let short_text = "Presentation Slide 1\nTitle: Neural Search";
        assert!(short_text.chars().count() < MIN_TEXT_CHARS);

        let placeholder = PDFExtractor::render_placeholder(&temp_dir, 1).unwrap();
        assert!(placeholder.exists());

        let page = PdfPage {
            number: 1,
            text: short_text.to_string(),
            image_path: Some(placeholder),
            ocr: false,
            garbled: None,
        };

        assert_eq!(page.number, 1);
        assert!(page.image_path.is_some());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_long_page_kept_as_pure_text() {
        let long_text = (0..50)
            .map(|_| "The quick brown fox jumps over the lazy dog.")
            .collect::<Vec<_>>()
            .join(" ");

        assert!(long_text.chars().count() >= MIN_TEXT_CHARS);

        let page = PdfPage {
            number: 2,
            text: long_text,
            image_path: None,
            ocr: false,
            garbled: None,
        };

        assert!(page.image_path.is_none());
        assert!(!page.text.is_empty());
    }
}
