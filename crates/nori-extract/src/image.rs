use crate::doc::ExtractError;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Max pixel boundary for photographic images (1,024 px for EmbeddingGemma vision encoder).
pub const PHOTO_MAX_PIXEL: u32 = 1024;

/// Max pixel boundary for UI screenshots (1,536 px for high-DPI text legibility).
pub const SCREENSHOT_MAX_PIXEL: u32 = 1536;

/// Minimum image dimension threshold (images where max(width, height) < 256 are discarded).
pub const MIN_IMAGE_SIDE: u32 = 256;

/// Downscaled, orientation-corrected image ready for visual embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub taken: Option<String>,
}

/// Extractor for photographic images, screenshots, and visual assets.
pub struct ImageExtractor;

impl ImageExtractor {
    /// Prepares an image for neural embedding:
    /// 1. Reads dimensions and rejects icons/thumbnails (< `min_side`).
    /// 2. Reads EXIF metadata (`DateTimeOriginal` and orientation).
    /// 3. Corrects orientation and downscales proportionally to fit `max_pixel`.
    /// 4. Writes normalized JPG or PNG to `output_dir`.
    pub fn prepare(
        input_path: &Path,
        max_pixel: u32,
        min_side: u32,
        output_dir: &Path,
        png: bool,
    ) -> Result<PreparedImage, ExtractError> {
        let exif = ExifReader::read(input_path);

        let reader = ImageReader::open(input_path)?
            .with_guessed_format()
            .map_err(|e| ExtractError::Unreadable(e.to_string()))?;

        let (orig_width, orig_height) = reader
            .into_dimensions()
            .map_err(|e| ExtractError::Unreadable(format!("can't read dimensions: {e}")))?;

        if orig_width.max(orig_height) < min_side {
            return Err(ExtractError::Unreadable(format!(
                "image too small ({}x{}, min {})",
                orig_width, orig_height, min_side
            )));
        }

        // Decode pixel buffer
        let mut img = image::open(input_path)
            .map_err(|e| ExtractError::Unreadable(format!("can't decode image: {e}")))?;

        // Apply EXIF orientation tag if present
        let orientation = exif.as_ref().map(|e| e.orientation).unwrap_or(1);
        img = Self::apply_orientation(img, orientation);

        let (cur_w, cur_h) = img.dimensions();
        let longest = cur_w.max(cur_h);

        if longest > max_pixel {
            let scale = max_pixel as f64 / longest as f64;
            let target_w = (cur_w as f64 * scale).round().max(1.0) as u32;
            let target_h = (cur_h as f64 * scale).round().max(1.0) as u32;
            img = img.resize(target_w, target_h, image::imageops::FilterType::Triangle);
        }

        let out_ext = if png { "png" } else { "jpg" };
        let out_path = output_dir.join(format!("{}.{}", Uuid::new_v4(), out_ext));

        let format = if png {
            ImageFormat::Png
        } else {
            ImageFormat::Jpeg
        };
        img.save_with_format(&out_path, format)
            .map_err(|e| ExtractError::Unreadable(format!("can't write image: {e}")))?;

        Ok(PreparedImage {
            path: out_path,
            width: orig_width,
            height: orig_height,
            taken: exif.and_then(|e| e.date_time_original),
        })
    }

    /// Rotates or flips the image according to standard EXIF orientation tags (1..=8).
    pub fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
        match orientation {
            2 => img.fliph(),
            3 => img.rotate180(),
            4 => img.flipv(),
            5 => img.rotate90().fliph(),
            6 => img.rotate90(),
            7 => img.rotate270().fliph(),
            8 => img.rotate270(),
            _ => img,
        }
    }
}

/// Parsed EXIF metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExifData {
    pub orientation: u32,
    pub date_time_original: Option<String>,
}

/// Lightweight, zero-dependency EXIF parser for JPEG and TIFF streams.
pub struct ExifReader;

impl ExifReader {
    pub fn read(path: &Path) -> Option<ExifData> {
        let mut file = File::open(path).ok()?;
        let mut magic = [0u8; 12];
        if file.read_exact(&mut magic).is_err() {
            return None;
        }

        // JPEG: starts with 0xFF 0xD8
        if magic[0] == 0xFF && magic[1] == 0xD8 {
            return Self::read_jpeg(&mut file);
        }

        // TIFF: starts with "II*\0" (little endian) or "MM\0*" (big endian)
        if (magic[0..4] == [0x49, 0x49, 0x2A, 0x00]) || (magic[0..4] == [0x4D, 0x4D, 0x00, 0x2A]) {
            file.seek(SeekFrom::Start(0)).ok()?;
            let mut data = Vec::new();
            file.take(1 << 20).read_to_end(&mut data).ok()?;
            return Self::parse_tiff(&data, 0);
        }

        None
    }

    fn read_jpeg(file: &mut File) -> Option<ExifData> {
        let mut offset = 2u64;

        for _ in 0..100 {
            file.seek(SeekFrom::Start(offset)).ok()?;
            let mut marker_buf = [0u8; 4];
            if file.read_exact(&mut marker_buf).is_err() {
                break;
            }

            if marker_buf[0] != 0xFF {
                break;
            }

            let marker = marker_buf[1];
            if marker == 0xDA || marker == 0xD9 {
                // SOS (Start of Scan) or EOI: end of metadata headers
                break;
            }

            let length = u16::from_be_bytes([marker_buf[2], marker_buf[3]]) as u64;
            if length < 2 {
                break;
            }

            // APP1 marker (0xFFE1): check for "Exif\0\0"
            if marker == 0xE1 {
                let mut app1_data = vec![0u8; (length - 2) as usize];
                if file.read_exact(&mut app1_data).is_ok() {
                    if app1_data.len() >= 6 && &app1_data[0..6] == b"Exif\0\0" {
                        return Self::parse_tiff(&app1_data[6..], 0);
                    }
                }
            }

            offset += 2 + length;
        }

        None
    }

    fn parse_tiff(data: &[u8], tiff_offset: usize) -> Option<ExifData> {
        if data.len() < tiff_offset + 8 {
            return None;
        }

        let slice = &data[tiff_offset..];
        let little_endian = match &slice[0..2] {
            [0x49, 0x49] => true,
            [0x4D, 0x4D] => false,
            _ => return None,
        };

        let read_u16 = |buf: &[u8], off: usize| -> Option<u16> {
            if off + 2 <= buf.len() {
                Some(if little_endian {
                    u16::from_le_bytes([buf[off], buf[off + 1]])
                } else {
                    u16::from_be_bytes([buf[off], buf[off + 1]])
                })
            } else {
                None
            }
        };

        let read_u32 = |buf: &[u8], off: usize| -> Option<u32> {
            if off + 4 <= buf.len() {
                Some(if little_endian {
                    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
                } else {
                    u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
                })
            } else {
                None
            }
        };

        let magic = read_u16(slice, 2)?;
        if magic != 42 {
            return None;
        }

        let ifd0_offset = read_u32(slice, 4)? as usize;
        if ifd0_offset >= slice.len() {
            return None;
        }

        let mut exif_data = ExifData {
            orientation: 1,
            date_time_original: None,
        };

        let num_entries = read_u16(slice, ifd0_offset)? as usize;
        let mut exif_ifd_offset: Option<usize> = None;

        for i in 0..num_entries {
            let entry_off = ifd0_offset + 2 + i * 12;
            if entry_off + 12 > slice.len() {
                break;
            }

            let tag = read_u16(slice, entry_off)?;
            if tag == 0x0112 {
                // Orientation tag
                if let Some(val) = read_u16(slice, entry_off + 8) {
                    exif_data.orientation = val as u32;
                }
            } else if tag == 0x8769 {
                // Exif SubIFD offset tag
                if let Some(val) = read_u32(slice, entry_off + 8) {
                    exif_ifd_offset = Some(val as usize);
                }
            }
        }

        // Parse Exif SubIFD if present for DateTimeOriginal
        if let Some(sub_offset) = exif_ifd_offset {
            if sub_offset + 2 <= slice.len() {
                if let Some(sub_entries) = read_u16(slice, sub_offset) {
                    for i in 0..sub_entries as usize {
                        let entry_off = sub_offset + 2 + i * 12;
                        if entry_off + 12 > slice.len() {
                            break;
                        }

                        let tag = read_u16(slice, entry_off)?;
                        if tag == 0x9003 {
                            // DateTimeOriginal
                            let count = read_u32(slice, entry_off + 4)? as usize;
                            let val_off = read_u32(slice, entry_off + 8)? as usize;
                            if val_off + count <= slice.len() && count > 1 {
                                let str_bytes = &slice[val_off..val_off + count];
                                let clean_str = String::from_utf8_lossy(str_bytes)
                                    .trim_end_matches('\0')
                                    .trim()
                                    .to_string();
                                if !clean_str.is_empty() {
                                    exif_data.date_time_original = Some(clean_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        Some(exif_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_image_prepare_downscale() {
        let temp_dir = std::env::temp_dir().join(format!("nori_img_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let input_path = temp_dir.join("large_photo.png");

        // 2000 x 1000 image
        let mut img = RgbImage::new(2000, 1000);
        for x in 0..2000 {
            for y in 0..1000 {
                img.put_pixel(x, y, Rgb([100, 150, 200]));
            }
        }
        img.save(&input_path).unwrap();

        let prepared = ImageExtractor::prepare(
            &input_path,
            PHOTO_MAX_PIXEL,
            MIN_IMAGE_SIDE,
            &temp_dir,
            false,
        )
        .unwrap();

        assert_eq!(prepared.width, 2000);
        assert_eq!(prepared.height, 1000);

        // Verify saved image downscaled to fit 1024px
        let saved = image::open(&prepared.path).unwrap();
        assert_eq!(saved.width(), 1024);
        assert_eq!(saved.height(), 512);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_image_prepare_too_small_rejected() {
        let temp_dir = std::env::temp_dir().join(format!("nori_small_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let input_path = temp_dir.join("tiny_icon.png");

        // 128 x 128 image (below MIN_IMAGE_SIDE 256)
        let img = RgbImage::new(128, 128);
        img.save(&input_path).unwrap();

        let res = ImageExtractor::prepare(
            &input_path,
            PHOTO_MAX_PIXEL,
            MIN_IMAGE_SIDE,
            &temp_dir,
            false,
        );

        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_exif_orientation_transform() {
        let mut img = RgbImage::new(100, 50);
        img.put_pixel(0, 0, Rgb([255, 0, 0])); // Red at top-left
        let dyn_img = DynamicImage::ImageRgb8(img);

        // Orientation 6: rotate 90° clockwise -> width becomes 50, height becomes 100
        let rotated = ImageExtractor::apply_orientation(dyn_img, 6);
        assert_eq!(rotated.width(), 50);
        assert_eq!(rotated.height(), 100);
    }
}
