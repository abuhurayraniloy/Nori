pub mod engine;
pub mod events;

pub use engine::{IndexError, Indexer};
pub use events::{IndexEvent, StepEvent, SyncSummary};

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::options::IndexOptions;
    use nori_infer::MockEmbedder;
    use nori_search::Searcher;
    use nori_storage::store::IndexStore;
    use std::fs::{create_dir_all, File};
    use std::io::Write;

    #[test]
    fn test_indexer_sync_and_run_end_to_end() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("nori_index_test_{nanos}"));
        create_dir_all(&temp_dir).unwrap();

        // 1. Create test markdown file
        let doc_file = temp_dir.join("quantum.md");
        File::create(&doc_file)
            .unwrap()
            .write_all(b"# Quantum Algorithms\nQuantum computers leverage superposition to factor large integers.")
            .unwrap();

        // 2. Create test code file
        let code_file = temp_dir.join("math.rs");
        File::create(&code_file)
            .unwrap()
            .write_all(b"pub fn fast_fourier_transform(signal: &[f32]) -> Vec<f32> {\n    vec![0.0]\n}\n")
            .unwrap();

        let store = IndexStore::open_in_memory().unwrap();
        let mut options = IndexOptions::default();
        options.code = false;
        options.include_code = true;

        let embedder = Box::new(MockEmbedder::new());
        let mut indexer = Indexer::new(store, options, embedder).unwrap();

        // 3. Sync phase
        let summary = indexer.sync(&[temp_dir.clone()]).unwrap();
        assert_eq!(summary.added, 2);
        assert_eq!(summary.queued.len(), 2);

        // 4. Indexing run phase
        let mut events = Vec::new();
        let states = indexer
            .run(None, None, || false, |ev| events.push(ev))
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(states.get("done"), Some(&2));

        // 5. Connect Searcher to the indexed store and search
        let store = indexer.finish();
        let mut searcher = Searcher::new(store, true, false).unwrap();
        let query_vec = MockEmbedder::new().deterministic_vector("superposition quantum");

        let hits = searcher
            .search("superposition", Some(&query_vec), None, 10, 0.5, None, false)
            .unwrap();

        assert!(!hits.is_empty());
        assert!(hits[0].path.contains("quantum.md"));
        assert!(hits[0].z.unwrap() > 0.0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
