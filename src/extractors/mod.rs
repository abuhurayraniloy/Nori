use std::{
    fmt, io,
    path::{Path, PathBuf},
};

mod pdf;

#[derive(Debug, PartialEq)]
pub struct ExtractedDocument {
    pub source_path: PathBuf,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum ExtractionError {
    Io(io::Error),
    Pdf(String),
    UnsupportedExtension(String),
    InputTooLarge { actual_bytes: u64, max_bytes: u64 },
    ExtractorPanicked,
    OcrDeferred,
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Pdf(error) => write!(formatter, "PDF extraction failed: {error}"),
            Self::UnsupportedExtension(error) => {
                write!(formatter, "Unsupported file extension {error}")
            }
            Self::InputTooLarge {
                actual_bytes,
                max_bytes,
            } => write!(
                formatter,
                "File is too large: {actual_bytes} bytes; maximum is: {max_bytes} bytes"
            ),
            Self::ExtractorPanicked => write!(formatter, "The extractor unexpectedly panicked"),
            Self::OcrDeferred => write!(formatter, "ImageOCR is not yet available"),
        }
    }
}

impl std::error::Error for ExtractionError {}

impl From<io::Error> for ExtractionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn extract_content(path: &Path) -> Result<ExtractedDocument, ExtractionError> {
    match std::panic::catch_unwind(|| extract_content_inner(path)) {
        Ok(result) => result,
        Err(_) => Err(ExtractionError::ExtractorPanicked),
    }
}

fn extract_content_inner(path: &Path) -> Result<ExtractedDocument, ExtractionError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            ExtractionError::UnsupportedExtension("missing or invalid extension".to_string())
        })?;

    match extension.as_str() {
        "pdf" => pdf::extract_pdf(path),

        "png" | "jpg" | "jpeg" | "bmp" | "webp" => Err(ExtractionError::OcrDeferred),

        other => Err(ExtractionError::UnsupportedExtension(other.to_string())),
    }
}
