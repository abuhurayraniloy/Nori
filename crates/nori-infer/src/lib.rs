pub mod ipc;
pub mod mock;
pub mod pooling;
pub mod sizing;
pub mod traits;

pub use ipc::{
    decode_f32_vectors, encode_f32_vectors, EmbeddingWorker, WorkerConfig, WorkerError,
    WorkerReply, WorkerRequest,
};
pub use mock::MockEmbedder;
pub use pooling::{l2_normalize, l2_normalized, mean_pool};
pub use sizing::ImageSizing;
pub use traits::{Embedder, EmbeddingResult, WorkerInfo};
