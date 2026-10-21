use crate::pooling::l2_normalize;
use crate::traits::{Embedder, EmbeddingResult, WorkerInfo};

/// Deterministic in-memory embedder for testing search, indexing, and ranking.
#[derive(Debug, Clone)]
pub struct MockEmbedder {
    pub info: WorkerInfo,
}

impl MockEmbedder {
    pub fn new() -> Self {
        Self::with_dim(768)
    }

    pub fn with_dim(dim: usize) -> Self {
        Self {
            info: WorkerInfo {
                model: "mock-embeddinggemma".to_string(),
                revision: "mockrev1234567".to_string(),
                runtime: "mock-runtime".to_string(),
                dtype: "f32".to_string(),
                dim,
                text_only: false,
                load_seconds: Some(0.001),
            },
        }
    }

    /// Generates a deterministic L2-normalized pseudo-vector of dimension `self.info.dim` for a string key.
    pub fn deterministic_vector(&self, key: &str) -> Vec<f32> {
        let mut vec = Vec::with_capacity(self.info.dim);
        let mut state: u64 = 0xcbf29ce484222325; // FNV-1a 64-bit offset basis
        for byte in key.bytes() {
            state ^= byte as u64;
            state = state.wrapping_mul(0x100000001b3);
        }

        // Generate pseudo-random floats using Xorshift64*
        for _ in 0..self.info.dim {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let rand_val = state.wrapping_mul(0x2545F4914F6CDD1D);
            let f = ((rand_val >> 32) as i32 as f32) / (i32::MAX as f32);
            vec.push(f);
        }

        l2_normalize(&mut vec);
        vec
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl Embedder for MockEmbedder {
    fn info(&self) -> &WorkerInfo {
        &self.info
    }

    fn embed_texts(
        &mut self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(texts.iter().map(|t| self.deterministic_vector(t)).collect())
    }

    fn embed_images(
        &mut self,
        paths: &[String],
        _budget: usize,
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(paths
            .iter()
            .map(|p| {
                if p.ends_with(".corrupt") || p.ends_with(".invalid") {
                    EmbeddingResult {
                        vector: None,
                        error: Some("Image decoding failed".to_string()),
                    }
                } else {
                    EmbeddingResult {
                        vector: Some(self.deterministic_vector(p)),
                        error: None,
                    }
                }
            })
            .collect())
    }

    fn embed_audio(
        &mut self,
        paths: &[String],
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(paths
            .iter()
            .map(|p| {
                if p.ends_with(".corrupt") || p.ends_with(".invalid") {
                    EmbeddingResult {
                        vector: None,
                        error: Some("Audio decoding failed".to_string()),
                    }
                } else {
                    EmbeddingResult {
                        vector: Some(self.deterministic_vector(p)),
                        error: None,
                    }
                }
            })
            .collect())
    }

    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_embedder_deterministic() {
        let mut embedder = MockEmbedder::new();
        let vec1 = embedder.deterministic_vector("hello world");
        let vec2 = embedder.deterministic_vector("hello world");
        let vec3 = embedder.deterministic_vector("different text");

        assert_eq!(vec1, vec2);
        assert_ne!(vec1, vec3);
        assert_eq!(vec1.len(), 768);

        // Verify L2 norm is ~1.0
        let norm_sq: f32 = vec1.iter().map(|x| x * x).sum();
        assert!((norm_sq - 1.0).abs() < 1e-4);

        // Test embed_texts
        let res = embedder
            .embed_texts(&["test1".into(), "test2".into()])
            .unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].len(), 768);
    }
}
