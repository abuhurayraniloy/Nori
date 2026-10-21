use crate::audio::MediaPiece;
use image::DynamicImage;
use std::path::PathBuf;

/// Default sampling interval between video candidate frames (3.0 seconds).
pub const DEFAULT_INTERVAL_SECS: f64 = 3.0;

/// Maximum number of sampled frames across a single video file (clamps 2+ hour videos).
pub const MAX_FRAMES: usize = 1200;

/// Maximum pixel dimension for saved keyframe images (1,024 px for EmbeddingGemma vision encoder).
pub const MAX_PIXEL: u32 = 1024;

/// Hamming distance threshold for 8x8 average hash deduplication (<= 4 bits difference = duplicate shot).
pub const HAMMING_THRESHOLD: u32 = 4;

/// Result of video extraction pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoExtractionResult {
    pub frames: Vec<MediaPiece>,
    pub duration: f64,
    pub has_audio: bool,
}

/// Keyframe sampler, 8x8 average perceptual hasher (aHash), and shot deduplication engine.
pub struct VideoExtractor;

impl VideoExtractor {
    /// Computes an 8x8 Average Hash (aHash) from an image, packed into a 64-bit integer.
    ///
    /// 1. Resizes image to 8x8 grayscale.
    /// 2. Computes the arithmetic mean luminance across all 64 pixels.
    /// 3. Sets bit `i` to 1 if pixel `i` > mean, 0 otherwise.
    pub fn average_hash(img: &DynamicImage) -> u64 {
        let thumb = img
            .resize_exact(8, 8, image::imageops::FilterType::Nearest)
            .to_luma8();
        let pixels = thumb.as_raw();

        let sum: u32 = pixels.iter().map(|&p| p as u32).sum();
        let mean = (sum / 64) as u8;

        let mut hash = 0u64;
        for (i, &pixel) in pixels.iter().enumerate() {
            if pixel > mean {
                hash |= 1u64 << i;
            }
        }
        hash
    }

    /// Computes the 8x8 Average Hash directly from raw RGB pixel data.
    pub fn average_hash_rgb(width: usize, height: usize, pixels: &[u8]) -> u64 {
        if width == 0 || height == 0 || pixels.is_empty() {
            return 0;
        }

        let side = 8;
        let mut cells = [0usize; 64];

        for cy in 0..side {
            let y0 = cy * height / side;
            let y1 = height.min((y0 + 1).max((cy + 1) * height / side));
            for cx in 0..side {
                let x0 = cx * width / side;
                let x1 = width.min((x0 + 1).max((cx + 1) * width / side));

                let mut sum = 0usize;
                let mut n = 0usize;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = (y * width + x) * 3;
                        if p + 2 < pixels.len() {
                            sum += (pixels[p] as usize * 299
                                + pixels[p + 1] as usize * 587
                                + pixels[p + 2] as usize * 114)
                                / 1000;
                            n += 1;
                        }
                    }
                }
                cells[cy * side + cx] = if n > 0 { sum / n } else { 0 };
            }
        }

        let mean: usize = cells.iter().sum::<usize>() / 64;
        let mut hash: u64 = 0;
        for (index, &cell) in cells.iter().enumerate() {
            if cell > mean {
                hash |= 1u64 << index;
            }
        }
        hash
    }

    /// Computes bitwise Hamming distance between two 64-bit perceptual hashes in a single CPU popcount instruction.
    #[inline(always)]
    pub fn hamming_distance(h1: u64, h2: u64) -> u32 {
        (h1 ^ h2).count_ones()
    }

    /// Checks if two hashes represent visually identical scenes (Hamming distance <= 4 bits).
    #[inline(always)]
    pub fn is_duplicate(h1: u64, h2: u64) -> bool {
        Self::hamming_distance(h1, h2) <= HAMMING_THRESHOLD
    }

    /// Calculates sampling timestamps for a video given duration and interval constraints.
    pub fn sample_timestamps(duration: f64, interval: f64, max_frames: usize) -> Vec<f64> {
        if duration <= 0.0 {
            return Vec::new();
        }

        let step = interval.max(duration / max_frames as f64);
        let first = 0.5f64.min(duration / 2.0);

        let mut timestamps = Vec::new();
        let mut t = first;
        while t < duration {
            timestamps.push(t);
            t += step;
        }
        timestamps
    }

    /// Assigns continuous temporal spans to retained keyframes.
    /// Each keyframe covers the full duration until the next scene cut, with the last frame ending at `duration`.
    pub fn assign_spans(frames: &mut [MediaPiece], duration: f64) {
        if frames.is_empty() {
            return;
        }
        let count = frames.len();
        for i in 0..count - 1 {
            frames[i].end = frames[i + 1].start;
        }
        if let Some(last) = frames.last_mut() {
            last.end = duration;
        }
    }

    /// Filters candidate frames using Hamming distance <= 4 and assigns contiguous spans.
    pub fn deduplicate_keyframes(
        candidates: &[(PathBuf, f64, u64)],
        duration: f64,
    ) -> Vec<MediaPiece> {
        let mut frames = Vec::new();
        let mut last_hash: Option<u64> = None;

        for (path, start, hash) in candidates {
            if let Some(prev) = last_hash {
                if Self::is_duplicate(*hash, prev) {
                    // Visual duplicate of the existing shot; discard
                    let _ = std::fs::remove_file(path);
                    continue;
                }
            }

            last_hash = Some(*hash);
            frames.push(MediaPiece {
                path: path.clone(),
                start: *start,
                end: duration,
            });
        }

        Self::assign_spans(&mut frames, duration);
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_ahash_identical_images() {
        let mut img = RgbImage::new(64, 64);
        for x in 0..32 {
            for y in 0..64 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        let hash1 = VideoExtractor::average_hash(&dyn_img);
        let hash2 = VideoExtractor::average_hash(&dyn_img);

        assert_eq!(hash1, hash2);
        assert_eq!(VideoExtractor::hamming_distance(hash1, hash2), 0);
        assert!(VideoExtractor::is_duplicate(hash1, hash2));
    }

    #[test]
    fn test_ahash_slight_variation_is_duplicate() {
        let mut img1 = RgbImage::new(64, 64);
        let mut img2 = RgbImage::new(64, 64);

        // Fill background with gray (128)
        for x in 0..64 {
            for y in 0..64 {
                img1.put_pixel(x, y, Rgb([128, 128, 128]));
                img2.put_pixel(x, y, Rgb([128, 128, 128]));
            }
        }

        // Add a bright vertical bar on left side of both
        for x in 0..16 {
            for y in 0..64 {
                img1.put_pixel(x, y, Rgb([250, 250, 250]));
                img2.put_pixel(x, y, Rgb([240, 240, 240])); // Tiny brightness difference
            }
        }

        let hash1 = VideoExtractor::average_hash(&DynamicImage::ImageRgb8(img1));
        let hash2 = VideoExtractor::average_hash(&DynamicImage::ImageRgb8(img2));

        let distance = VideoExtractor::hamming_distance(hash1, hash2);
        assert!(distance <= HAMMING_THRESHOLD);
        assert!(VideoExtractor::is_duplicate(hash1, hash2));
    }

    #[test]
    fn test_ahash_different_scenes_not_duplicate() {
        let mut img_left = RgbImage::new(64, 64);
        let mut img_right = RgbImage::new(64, 64);

        // Image 1: left half white, right half black
        for x in 0..64 {
            for y in 0..64 {
                if x < 32 {
                    img_left.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    img_left.put_pixel(x, y, Rgb([0, 0, 0]));
                }
            }
        }

        // Image 2: top half white, bottom half black
        for x in 0..64 {
            for y in 0..64 {
                if y < 32 {
                    img_right.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    img_right.put_pixel(x, y, Rgb([0, 0, 0]));
                }
            }
        }

        let hash1 = VideoExtractor::average_hash(&DynamicImage::ImageRgb8(img_left));
        let hash2 = VideoExtractor::average_hash(&DynamicImage::ImageRgb8(img_right));

        let distance = VideoExtractor::hamming_distance(hash1, hash2);
        assert!(distance > HAMMING_THRESHOLD);
        assert!(!VideoExtractor::is_duplicate(hash1, hash2));
    }

    #[test]
    fn test_sample_timestamps_clamping() {
        // Short video: 10s duration, 3s interval -> samples at 0.5, 3.5, 6.5, 9.5
        let ts = VideoExtractor::sample_timestamps(10.0, 3.0, 1200);
        assert_eq!(ts.len(), 4);
        assert_eq!(ts[0], 0.5);
        assert_eq!(ts[1], 3.5);
        assert_eq!(ts[2], 6.5);
        assert_eq!(ts[3], 9.5);

        // Long video: 7200s (2 hours) with max_frames = 1200
        // step = max(3.0, 7200 / 1200) = 6.0s
        let ts_long = VideoExtractor::sample_timestamps(7200.0, 3.0, 1200);
        assert!(ts_long.len() <= 1200);
        assert_eq!(ts_long[0], 0.5);
        assert_eq!(ts_long[1], 6.5);
    }

    #[test]
    fn test_assign_continuous_spans() {
        let mut frames = vec![
            MediaPiece {
                path: PathBuf::from("frame1.jpg"),
                start: 0.5,
                end: 10.0,
            },
            MediaPiece {
                path: PathBuf::from("frame2.jpg"),
                start: 3.5,
                end: 10.0,
            },
            MediaPiece {
                path: PathBuf::from("frame3.jpg"),
                start: 7.0,
                end: 10.0,
            },
        ];

        VideoExtractor::assign_spans(&mut frames, 10.0);

        // Frame 1 extends to Frame 2's start
        assert_eq!(frames[0].start, 0.5);
        assert_eq!(frames[0].end, 3.5);

        // Frame 2 extends to Frame 3's start
        assert_eq!(frames[1].start, 3.5);
        assert_eq!(frames[1].end, 7.0);

        // Frame 3 extends to total duration
        assert_eq!(frames[2].start, 7.0);
        assert_eq!(frames[2].end, 10.0);
    }

    #[test]
    fn test_raw_rgb_hash_parity() {
        // Test 10x10 RGB buffer
        let width = 16;
        let height = 16;
        let mut rgb = vec![0u8; width * height * 3];
        // Make top-left quadrant white
        for y in 0..8 {
            for x in 0..8 {
                let idx = (y * width + x) * 3;
                rgb[idx] = 255;
                rgb[idx + 1] = 255;
                rgb[idx + 2] = 255;
            }
        }

        let hash = VideoExtractor::average_hash_rgb(width, height, &rgb);
        assert_ne!(hash, 0);

        // The top-left 4x4 cells of the 8x8 grid should have bit set to 1
        assert_eq!(hash & 0x0F0F_0F0F, 0x0F0F_0F0F);
    }
}
