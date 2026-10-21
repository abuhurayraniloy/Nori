/// Mean pooling across token representations and L2 unit length normalization.

/// Computes mean pooling across a contiguous slice of token embeddings.
/// `token_embeddings` is laid out as `num_tokens * dim` elements.
pub fn mean_pool(token_embeddings: &[f32], num_tokens: usize, dim: usize) -> Vec<f32> {
    if num_tokens == 0 || dim == 0 || token_embeddings.len() < num_tokens * dim {
        return vec![0.0; dim];
    }

    let mut pooled = vec![0.0f32; dim];
    for t in 0..num_tokens {
        let offset = t * dim;
        for i in 0..dim {
            pooled[i] += token_embeddings[offset + i];
        }
    }

    let scale = 1.0 / num_tokens as f32;
    for x in &mut pooled {
        *x *= scale;
    }

    pooled
}

/// Normalizes a vector in-place to unit Euclidean length (L2 norm = 1.0).
pub fn l2_normalize(vector: &mut [f32]) {
    let sum_sq: f64 = vector.iter().map(|&x| (x as f64) * (x as f64)).sum();
    if sum_sq > 0.0 {
        let norm = sum_sq.sqrt() as f32;
        let inv_norm = 1.0 / norm;
        for x in vector {
            *x *= inv_norm;
        }
    }
}

/// Returns a new L2 unit normalized copy of `vector`.
pub fn l2_normalized(vector: &[f32]) -> Vec<f32> {
    let mut copy = vector.to_vec();
    l2_normalize(&mut copy);
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean_pool_calculation() {
        let dim = 4;
        let num_tokens = 3;
        let tokens = vec![
            1.0, 2.0, 3.0, 4.0, // token 0
            2.0, 4.0, 6.0, 8.0, // token 1
            3.0, 6.0, 9.0, 12.0, // token 2
        ];

        let pooled = mean_pool(&tokens, num_tokens, dim);
        assert_eq!(pooled, vec![2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn test_l2_normalization() {
        let mut v = vec![3.0f32, 4.0f32];
        l2_normalize(&mut v);

        // Norm should be sqrt(3^2 + 4^2) = 5 -> [0.6, 0.8]
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);

        let len_sq: f32 = v.iter().map(|x| x * x).sum();
        assert!((len_sq - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_zero_vector_normalization() {
        let mut v = vec![0.0f32; 768];
        l2_normalize(&mut v);
        assert!(v.iter().all(|&x| x == 0.0));
    }
}
