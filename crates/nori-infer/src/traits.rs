use serde::{Deserialize, Serialize};

/// Runtime model information reported by the inference engine (forms part of index fingerprint).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub model: String,
    pub revision: String,
    pub runtime: String,
    pub dtype: String,
    pub dim: usize,
    pub text_only: bool,
    pub load_seconds: Option<f64>,
}

impl WorkerInfo {
    /// Vectors from different models, revisions, runtimes or precisions must never share an index.
    pub fn vector_source(&self) -> String {
        let rev = if self.revision.len() >= 7 {
            &self.revision[..7]
        } else {
            &self.revision
        };
        format!("{}@{} {} {}", self.model, rev, self.runtime, self.dtype)
    }
}

/// A multimodal embedding result for an asset (vector or failure reason).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingResult {
    pub vector: Option<Vec<f32>>,
    pub error: Option<String>,
}

/// The common trait implemented by in-process embedders or out-of-process worker clients.
pub trait Embedder: Send {
    /// Returns model runtime metadata.
    fn info(&self) -> &WorkerInfo;

    /// Embeds a batch of prompt-formatted text inputs into L2-normalized vectors.
    fn embed_texts(
        &mut self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>>;

    /// Embeds image files into L2-normalized vectors using soft-token visual budgeting.
    fn embed_images(
        &mut self,
        paths: &[String],
        budget: usize,
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>>;

    /// Embeds 16 kHz audio window files into L2-normalized vectors.
    fn embed_audio(
        &mut self,
        paths: &[String],
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>>;

    /// Shuts down the embedder process/context cleanly.
    fn close(&mut self);
}
