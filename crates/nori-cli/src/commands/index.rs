//! Command: nori index <folder>...

use nori_core::options::IndexOptions;
use nori_index::Indexer;
use nori_infer::{EmbeddingWorker, MockEmbedder, WorkerConfig};
use nori_storage::store::IndexStore;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct IndexArgs {
    pub folders: Vec<PathBuf>,
    pub is_code: bool,
    pub db_path: Option<String>,
    pub limit: Option<usize>,
}

pub fn run_index(args: IndexArgs) -> Result<(), String> {
    if args.folders.is_empty() {
        return Err("No folders specified for indexing. Usage: nori index <folder>...".into());
    }

    let default_db = if args.is_code {
        "nori_code.sqlite"
    } else {
        "nori.sqlite"
    };
    let db_path = args.db_path.as_deref().unwrap_or(default_db);

    let store = IndexStore::open(Path::new(db_path))
        .map_err(|e| format!("failed to open database '{db_path}': {e}"))?;

    let embedder: Box<dyn nori_infer::traits::Embedder> =
        if let Some(Ok(config)) = WorkerConfig::from_environment() {
            if let Ok(worker) = EmbeddingWorker::new(config, true) {
                Box::new(worker)
            } else {
                Box::new(MockEmbedder::new())
            }
        } else {
            Box::new(MockEmbedder::new())
        };

    let options = IndexOptions {
        code: args.is_code,
        ..Default::default()
    };

    let mut indexer = Indexer::new(store, options, embedder)
        .map_err(|e| format!("failed to initialize indexer: {e}"))?;

    println!(
        "🚀 Starting indexing on {} folder(s) (is_code: {})...",
        args.folders.len(),
        args.is_code
    );

    let start_time = Instant::now();
    let sync_summary = indexer
        .sync(&args.folders)
        .map_err(|e| format!("sync failed: {e}"))?;

    println!(
        "📂 Sync complete: {} added, {} changed, {} moved, {} removed, {} unchanged",
        sync_summary.added,
        sync_summary.changed,
        sync_summary.moved,
        sync_summary.removed,
        sync_summary.unchanged
    );

    let states = indexer
        .run(args.limit, None, || false, |event| {
            if event.done % 10 == 0 || event.done == event.total {
                print!(
                    "\r   Progress: {}/{} [{:?}] {}",
                    event.done,
                    event.total,
                    event.kind,
                    Path::new(&event.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| event.path.clone())
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        })
        .map_err(|e| format!("indexing run failed: {e}"))?;

    println!("\n🧩 Finalizing chunks and long files...");
    let _ = indexer.finish_long_files(|| false, |_| {});

    let elapsed = start_time.elapsed();
    let total_indexed: usize = states.values().sum();
    println!(
        "✅ Indexed {} file(s) in {:.2}s. Database: {}",
        total_indexed,
        elapsed.as_secs_f32(),
        db_path
    );

    Ok(())
}
