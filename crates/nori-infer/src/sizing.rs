/// Soft-token adaptive image sizing and PIL-compatible bicubic resampling.
///
/// EmbeddingGemma's vision encoder operates on a fixed budget of soft tokens
/// (each composed of 3x3 pooling over 16px patches = 48x48 px blocks).
pub struct ImageSizing;

const PRECISION: i32 = 22;

impl ImageSizing {
    /// Supported soft-token budgets: 70, 140, 280, 560, 1120.
    pub const BUDGETS: &[usize] = &[70, 140, 280, 560, 1120];

    /// Calculates target dimensions scaled to fill `budget` soft tokens while preserving aspect ratio,
    /// rounding each side down to multiples of 48 px.
    pub fn target_size(height: usize, width: usize, budget: usize) -> Option<(usize, usize)> {
        if height == 0 || width == 0 {
            return None;
        }

        let patch = 16usize;
        let pooling = 3usize;
        let max_patches = budget * pooling * pooling;
        let factor = ((max_patches * patch * patch) as f64 / (height * width) as f64).sqrt();
        let side = pooling * patch; // 48

        let mut target_height = ((factor * height as f64 / side as f64).floor() as usize) * side;
        let mut target_width = ((factor * width as f64 / side as f64).floor() as usize) * side;

        if target_height == 0 && target_width == 0 {
            return None;
        }

        let max_side = (max_patches / (pooling * pooling)) * side;
        if target_height == 0 {
            target_height = side;
            target_width = (width / height * side).min(max_side);
        } else if target_width == 0 {
            target_width = side;
            target_height = (height / width * side).min(max_side);
        }

        Some((target_height, target_width))
    }

    /// PIL-compatible bicubic resampling of packed 8-bit RGB image data (a = -0.5, 22-bit fixed point).
    pub fn resize_bicubic(
        source: &[u8],
        width: usize,
        height: usize,
        to_width: usize,
        to_height: usize,
    ) -> Vec<u8> {
        match (to_width != width, to_height != height) {
            (true, true) => {
                let pass = horizontal(source, width, height, to_width);
                vertical(&pass, height, to_width, to_height)
            }
            (true, false) => horizontal(source, width, height, to_width),
            (false, true) => vertical(source, height, width, to_height),
            (false, false) => source.to_vec(),
        }
    }
}

fn filter(value: f64) -> f64 {
    let x = value.abs();
    let a = -0.5;
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

fn coefficients(in_size: usize, out_size: usize) -> (Vec<usize>, Vec<i32>, usize) {
    let scale = in_size as f64 / out_size as f64;
    let filter_scale = scale.max(1.0);
    let support = 2.0 * filter_scale;
    let size = (support.ceil() as usize) * 2 + 1;

    let mut bounds = vec![0usize; out_size * 2];
    let mut weights = vec![0i32; out_size * size];
    let mut row = vec![0.0f64; size];

    for out in 0..out_size {
        let center = (out as f64 + 0.5) * scale;
        let first = (center - support + 0.5).max(0.0) as usize;
        let last = (center + support + 0.5).min(in_size as f64) as usize;
        let count = last.saturating_sub(first);

        let mut total = 0.0;
        for x in 0..size {
            row[x] = if x < count {
                filter(((x + first) as f64 - center + 0.5) / filter_scale)
            } else {
                0.0
            };
            total += row[x];
        }

        for x in 0..size {
            let weight = if x < count && total != 0.0 {
                row[x] / total
            } else {
                row[x]
            };
            let quantized =
                weight * ((1 << PRECISION) as f64) + if weight < 0.0 { -0.5 } else { 0.5 };
            weights[out * size + x] = quantized as i32;
        }

        bounds[out * 2] = first;
        bounds[out * 2 + 1] = count;
    }

    (bounds, weights, size)
}

#[inline(always)]
fn clip(value: i32) -> u8 {
    (value >> PRECISION).clamp(0, 255) as u8
}

fn horizontal(src: &[u8], in_width: usize, rows: usize, out_width: usize) -> Vec<u8> {
    let (bounds, weights, size) = coefficients(in_width, out_width);
    let mut out = vec![0u8; out_width * rows * 3];

    for y in 0..rows {
        for x in 0..out_width {
            let first = bounds[x * 2];
            let count = bounds[x * 2 + 1];
            let p_start = (y * in_width + first) * 3;

            let mut r: i32 = 1 << (PRECISION - 1);
            let mut g = r;
            let mut b = r;

            for k in 0..count {
                let w = weights[x * size + k];
                let p = p_start + k * 3;
                if p + 2 < src.len() {
                    r = r.wrapping_add((src[p] as i32).wrapping_mul(w));
                    g = g.wrapping_add((src[p + 1] as i32).wrapping_mul(w));
                    b = b.wrapping_add((src[p + 2] as i32).wrapping_mul(w));
                }
            }

            let o = (y * out_width + x) * 3;
            out[o] = clip(r);
            out[o + 1] = clip(g);
            out[o + 2] = clip(b);
        }
    }
    out
}

fn vertical(src: &[u8], in_height: usize, row_width: usize, out_height: usize) -> Vec<u8> {
    let (bounds, weights, size) = coefficients(in_height, out_height);
    let stride = row_width * 3;
    let mut out = vec![0u8; stride * out_height];

    for y in 0..out_height {
        let first = bounds[y * 2];
        let count = bounds[y * 2 + 1];
        let mut sums = vec![1i32 << (PRECISION - 1); stride];

        for k in 0..count {
            let row_start = (first + k) * stride;
            let w = weights[y * size + k];
            for i in 0..stride {
                if row_start + i < src.len() {
                    sums[i] = sums[i].wrapping_add((src[row_start + i] as i32).wrapping_mul(w));
                }
            }
        }

        for i in 0..stride {
            out[y * stride + i] = clip(sums[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_size_preserves_multiples_of_48() {
        // Budget 140 soft tokens
        let size = ImageSizing::target_size(1080, 1920, 140).unwrap();
        assert_eq!(size.0 % 48, 0);
        assert_eq!(size.1 % 48, 0);

        // Aspect ratio roughly 16:9
        let ratio = size.1 as f64 / size.0 as f64;
        assert!((ratio - (1920.0 / 1080.0)).abs() < 0.2);
    }

    #[test]
    fn test_target_size_square() {
        let size = ImageSizing::target_size(1000, 1000, 140).unwrap();
        assert_eq!(size.0, size.1);
        assert_eq!(size.0 % 48, 0);
    }

    #[test]
    fn test_resize_bicubic_dimensions() {
        let src_w = 4;
        let src_h = 4;
        let mut src = vec![0u8; src_w * src_h * 3];
        // Fill with white
        for b in &mut src {
            *b = 200;
        }

        let dst_w = 8;
        let dst_h = 8;
        let resized = ImageSizing::resize_bicubic(&src, src_w, src_h, dst_w, dst_h);
        assert_eq!(resized.len(), dst_w * dst_h * 3);
        // Average value should remain close to 200
        let avg: u32 = resized.iter().map(|&x| x as u32).sum::<u32>() / resized.len() as u32;
        assert!((avg as i32 - 200).abs() <= 2);
    }
}
