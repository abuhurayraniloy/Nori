pub mod hit;
pub mod ranking;
pub mod searcher;

pub use hit::SearchHit;
pub use ranking::{
    fts_query, is_lookup, kind_hints, meaning, modality_means, patterns, strong_count, terms,
    KIND_WORDS, STOPWORDS,
};
pub use searcher::{SearchError, Searcher};

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::types::{FileKind, Modality, SegmentKind};
    use nori_infer::MockEmbedder;
    use nori_storage::store::IndexStore;

    #[test]
    fn test_searcher_keyword_and_vector_fusion() {
        let mut store = IndexStore::open_in_memory().unwrap();
        let embedder = MockEmbedder::new();

        // 1. Insert documents
        let f1 = store
            .insert_file("/docs/quantum.md", FileKind::Doc, 2048, 100.0, 1, 1)
            .unwrap();
        let f2 = store
            .insert_file("/shots/error_dialog.png", FileKind::Screenshot, 1024, 200.0, 2, 1)
            .unwrap();

        // Generate deterministic vectors
        let vec_quantum = embedder.deterministic_vector("quantum computing algorithms");
        let vec_shot = embedder.deterministic_vector("application runtime error screenshot");

        // 2. Add segments with text and vectors
        let _ = store
            .add_segment(
                f1,
                SegmentKind::Chunk,
                Modality::Text,
                Some(1.0),
                Some(1.0),
                Some("quantum excerpt"),
                Some("Quantum computing utilizes qubits for exponential speedup in optimization algorithms."),
                Some(&vec_quantum),
            )
            .unwrap();

        let _ = store
            .add_segment(
                f2,
                SegmentKind::Image,
                Modality::Image,
                None,
                None,
                Some("error dialog"),
                Some("Fatal error 0x80004005 failed to allocate device memory"),
                Some(&vec_shot),
            )
            .unwrap();

        // 3. Initialize Searcher
        let mut searcher = Searcher::new(store, true, false).unwrap();
        assert_eq!(searcher.vector_count(), 2);

        // 4. Keyword search (without vector)
        let hits_kw = searcher
            .search("qubits optimization", None, None, 10, 2.0, None, false)
            .unwrap();
        assert!(!hits_kw.is_empty());
        assert_eq!(hits_kw[0].path, "/docs/quantum.md");
        assert!(hits_kw[0].excerpt.as_ref().unwrap().contains("«"));

        // 5. Semantic vector search (querying quantum)
        let query_vec = embedder.deterministic_vector("quantum computing algorithms");
        let hits_vec = searcher
            .search("quantum", Some(&query_vec), None, 10, 0.5, None, false)
            .unwrap();
        assert!(!hits_vec.is_empty());
        assert_eq!(hits_vec[0].path, "/docs/quantum.md");
        assert!(hits_vec[0].z.unwrap() > 0.0);
        assert!(hits_vec[0].cosine.unwrap() > 0.99); // identical vector

        // 6. Test strong_count
        let strong = strong_count(&hits_vec, 0.5);
        assert!(strong >= 1);
    }

    #[test]
    fn test_searcher_code_definition_focus() {
        let mut store = IndexStore::open_in_memory().unwrap();

        let f_code = store
            .insert_file("/repo/math.rs", FileKind::Code, 4096, 50.0, 10, 1)
            .unwrap();

        let code_body = r#"
// Helper comments
use std::fmt;

pub fn calculate_entropy(data: &[f32]) -> f32 {
    let mut sum = 0.0;
    for &x in data {
        if x > 0.0 {
            sum -= x * x.log2();
        }
    }
    sum
}
"#;

        let _ = store
            .add_segment(
                f_code,
                SegmentKind::Chunk,
                Modality::Text,
                Some(1.0),
                Some(14.0),
                None,
                Some(code_body),
                None,
            )
            .unwrap();

        // Code searcher
        let mut searcher = Searcher::new(store, false, true).unwrap();

        // Query definition of calculate_entropy
        let hits = searcher
            .search("calculate_entropy()", None, None, 10, 2.0, None, false)
            .unwrap();

        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "/repo/math.rs");
        // Line where `pub fn calculate_entropy` is defined (line 5)
        assert_eq!(hits[0].focus, Some(5.0));
    }
}
