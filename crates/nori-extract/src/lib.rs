//! Ingestion rules, file format filters, and multimodal extraction pipelines for Nori.

pub mod audio;
pub mod code;
pub mod doc;
pub mod folders;
pub mod image;
pub mod pdf;
pub mod rules;
pub mod video;

pub use audio::{
    AudioExtractError, AudioExtractor, MediaPiece, HOP_SECS, MIN_TAIL_SECS, RMS_SILENCE_THRESHOLD,
    SAMPLE_RATE, WINDOW_SECS,
};
pub use code::{CodeExtractor, CodeFile, CodeLookup, CodePiece};
pub use doc::{excerpt, DocExtractor, ExtractError, RTF};
pub use folders::{FolderSelection, Mark};
pub use image::{
    ExifData, ExifReader, ImageExtractor, PreparedImage, MIN_IMAGE_SIDE, PHOTO_MAX_PIXEL,
    SCREENSHOT_MAX_PIXEL,
};
pub use pdf::{
    PDFExtractor, PdfExtractionResult, PdfPage, FIRST_PASS_MAX_PAGES, MAX_PDF_BYTES,
    MIN_TEXT_CHARS, RENDER_PIXELS,
};
pub use rules::{Classification, Rules};
pub use video::{
    VideoExtractionResult, VideoExtractor, DEFAULT_INTERVAL_SECS, HAMMING_THRESHOLD, MAX_FRAMES,
    MAX_PIXEL,
};
