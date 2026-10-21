//! Command: nori search <words>...

use nori_core::types::FileKind;
use nori_infer::{EmbeddingWorker, MockEmbedder, WorkerConfig};
use nori_search::Searcher;
use nori_storage::store::IndexStore;
use std::collections::HashSet;
use std::path::Path;

pub struct SearchArgs {
    pub words: Vec<String>,
    pub limit: usize,
    pub is_code: bool,
    pub json: bool,
    pub kinds: Option<Vec<String>>,
    pub db_path: Option<String>,
}

pub fn run_search(args: SearchArgs) -> Result<(), String> {
    if args.words.is_empty() {
        return Err("No search query provided. Usage: nori search <words>...".into());
    }

    let default_db = if args.is_code {
        "nori_code.sqlite"
    } else {
        "nori.sqlite"
    };
    let db_path = args.db_path.as_deref().unwrap_or(default_db);

    let store = IndexStore::open(Path::new(db_path))
        .map_err(|e| format!("failed to open database '{db_path}': {e}"))?;

    let mut searcher = Searcher::new(store, true, args.is_code)
        .map_err(|e| format!("failed to initialize searcher: {e}"))?;

    let query = args.words.join(" ");

    let mut embedder: Box<dyn nori_infer::traits::Embedder> =
        if let Some(Ok(config)) = WorkerConfig::from_environment() {
            if let Ok(worker) = EmbeddingWorker::new(config, true) {
                Box::new(worker)
            } else {
                Box::new(MockEmbedder::new())
            }
        } else {
            Box::new(MockEmbedder::new())
        };

    let query_vec = match embedder.embed_texts(&[query.clone()]) {
        Ok(mut vs) => vs.pop(),
        Err(_) => None,
    };

    let filter_kinds: Option<HashSet<FileKind>> = args.kinds.map(|ks| {
        ks.iter()
            .filter_map(|k| match k.to_lowercase().as_str() {
                "screenshot" => Some(FileKind::Screenshot),
                "image" => Some(FileKind::Image),
                "pdf" => Some(FileKind::Pdf),
                "doc" => Some(FileKind::Doc),
                "audio" => Some(FileKind::Audio),
                "video" => Some(FileKind::Video),
                _ => None,
            })
            .collect()
    });

    let hits = searcher
        .search(
            &query,
            query_vec.as_deref(),
            filter_kinds.as_ref(),
            args.limit,
            1.5,
            None,
            false,
        )
        .map_err(|e| format!("search error: {e}"))?;

    if args.json {
        let json_hits: Vec<serde_json::Value> = hits
            .iter()
            .map(|h| {
                serde_json::json!({
                    "path": h.path,
                    "score": h.score,
                    "kind": format!("{:?}", h.kind),
                    "excerpt": h.excerpt,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_hits).unwrap());
    } else if hits.is_empty() {
        println!("No matching documents found for: \"{}\"", query);
    } else {
        println!("\n🔍 Found {} result(s) for \"{}\":\n", hits.len(), query);
        for (i, hit) in hits.iter().enumerate() {
            println!(
                "  {:2}. [{:4.2}] {:<12} {}",
                i + 1,
                hit.score,
                format!("[{:?}]", hit.kind),
                hit.path
            );
            if let Some(ref excerpt) = hit.excerpt {
                println!("      \"{}\"\n", excerpt.trim());
            }
        }
    }

    Ok(())
}
