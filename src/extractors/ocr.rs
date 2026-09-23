use super::{ExtractedDocument, ExtractionError};
use std::path::Path;

use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024; // 50 MB safety cap

pub(super) fn extract_image(path: &Path) -> Result<ExtractedDocument, ExtractionError> {
    // 1. File size sanity check
    let metadata = path.metadata()?;
    let actual_bytes = metadata.len();

    if actual_bytes > MAX_IMAGE_BYTES {
        return Err(ExtractionError::InputTooLarge {
            actual_bytes,
            max_bytes: MAX_IMAGE_BYTES,
        });
    }

    // 2. Read raw image bytes via standard Rust I/O
    let image_bytes = std::fs::read(path)?;

    // 3. Populate a WinRT InMemoryRandomAccessStream
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to create memory stream: {e}")))?;

    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|e| ExtractionError::Ocr(format!("Failed to create data writer: {e}")))?;

    writer
        .WriteBytes(&image_bytes)
        .map_err(|e| ExtractionError::Ocr(format!("Failed to write image bytes: {e}")))?;

    // Synchronously block using .join() from windows-future
    writer
        .StoreAsync()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to store stream: {e}")))?
        .join()
        .map_err(|e| ExtractionError::Ocr(format!("Stream store failed: {e}")))?;

    writer
        .FlushAsync()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to flush stream: {e}")))?
        .join()
        .map_err(|e| ExtractionError::Ocr(format!("Stream flush failed: {e}")))?;

    writer
        .DetachStream()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to detach stream: {e}")))?;

    stream
        .Seek(0)
        .map_err(|e| ExtractionError::Ocr(format!("Failed to rewind stream: {e}")))?;

    // 4. Decode into a SoftwareBitmap
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| ExtractionError::Ocr(format!("Unsupported or invalid image format: {e}")))?
        .join()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to decode image: {e}")))?;

    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to get software bitmap: {e}")))?
        .join()
        .map_err(|e| ExtractionError::Ocr(format!("Bitmap decoding failed: {e}")))?;

    // 5. Windows OCR strictly requires Bgra8 + Premultiplied alpha
    let bitmap = if bitmap
        .BitmapPixelFormat()
        .map_err(|e| ExtractionError::Ocr(e.to_string()))?
        != BitmapPixelFormat::Bgra8
        || bitmap
            .BitmapAlphaMode()
            .map_err(|e| ExtractionError::Ocr(e.to_string()))?
            != BitmapAlphaMode::Premultiplied
    {
        SoftwareBitmap::ConvertWithAlpha(
            &bitmap,
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
        )
        .map_err(|e| ExtractionError::Ocr(format!("Pixel format conversion to Bgra8 failed: {e}")))?
    } else {
        bitmap
    };

    // 6. Initialize OCR Engine with user's languages (with fallback)
    let engine = get_ocr_engine()?;

    // 7. Verify image dimensions do not exceed engine capacity
    let max_dim = OcrEngine::MaxImageDimension()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to query MaxImageDimension: {e}")))?;

    let width = bitmap.PixelWidth().map_err(|e| ExtractionError::Ocr(e.to_string()))? as u32;
    let height = bitmap.PixelHeight().map_err(|e| ExtractionError::Ocr(e.to_string()))? as u32;

    if width > max_dim || height > max_dim {
        return Err(ExtractionError::Ocr(format!(
            "Image dimensions ({width}x{height}) exceed maximum supported dimension ({max_dim}px)"
        )));
    }

    // 8. Run OCR Recognition
    let ocr_result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| ExtractionError::Ocr(format!("Failed to start OCR recognition: {e}")))?
        .join()
        .map_err(|e| ExtractionError::Ocr(format!("OCR recognition failed: {e}")))?;

    let text = ocr_result
        .Text()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to read OCR text: {e}")))?
        .to_string();

    Ok(ExtractedDocument {
        source_path: path.to_path_buf(),
        text,
        truncated: false,
    })
}

/// Attempts user profile languages first, then falls back to any installed OCR language pack
fn get_ocr_engine() -> Result<OcrEngine, ExtractionError> {
    if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(engine);
    }

    // Fallback: check all available installed recognizer languages
    let available_langs = OcrEngine::AvailableRecognizerLanguages()
        .map_err(|e| ExtractionError::Ocr(format!("Failed to query OCR languages: {e}")))?;

    if let Ok(first_lang) = available_langs.GetAt(0) {
        if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&first_lang) {
            return Ok(engine);
        }
    }

    Err(ExtractionError::Ocr(
        "No supported OCR language packs found on this Windows system. Please install one in Windows Settings -> Time & Language -> Language.".to_string(),
    ))
}