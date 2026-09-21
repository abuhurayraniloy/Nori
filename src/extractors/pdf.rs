use super::{ExtractedDocument, ExtractionError};
use std::path::Path;

const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 10;

pub(super) fn extract_pdf(path: &Path) -> Result<ExtractedDocument, ExtractionError> {
    let metadata = path.metadata()?;
    let actual_bytes = metadata.len();

    if actual_bytes > MAX_PDF_BYTES {
        return Err(ExtractionError::InputTooLarge {
            actual_bytes,
            max_bytes: MAX_PDF_BYTES,
        });
    }

    let mut document = pdf_extract::Document::load(&path)
        .map_err(|error| ExtractionError::Pdf(error.to_string()))?;

    if document.is_encrypted() {
        document
            .decrypt("")
            .map_err(|error| ExtractionError::Pdf(error.to_string()))?;
    }

    let pages = document.get_pages();
    let truncated = pages.len() > MAX_PDF_PAGES;

    let page_numbers: Vec<u32> = pages.keys().copied().take(MAX_PDF_PAGES).collect();

    if page_numbers.is_empty() {
        return Err(ExtractionError::Pdf(
            "the PDF contains no extractable pages!".to_string(),
        ));
    }

    let mut text = String::new();

    {
        let mut output = pdf_extract::PlainTextOutput::new(&mut text);

        for page_number in page_numbers {
            pdf_extract::output_doc_page(&document, &mut output, page_number)
                .map_err(|error| ExtractionError::Pdf(error.to_string()))?;
        }
    }

    Ok(ExtractedDocument {
        source_path: path.to_path_buf(),
        text,
        truncated,
    })
}
