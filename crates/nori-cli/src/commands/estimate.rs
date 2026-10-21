//! Command: nori estimate <folder>... [--code] [--json]

use nori_core::options::IndexOptions;
use nori_core::types::FileKind;
use nori_crawl::estimator::{Estimate, Estimator};
use nori_crawl::Crawler;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub struct EstimateArgs {
    pub folders: Vec<PathBuf>,
    pub is_code: bool,
    pub json: bool,
}

fn to_files_map(map: &HashMap<FileKind, usize>) -> HashMap<String, usize> {
    map.iter().map(|(k, v)| (k.as_str().to_string(), *v)).collect()
}

fn to_kind_seconds_map(map: &HashMap<FileKind, f64>) -> HashMap<String, f64> {
    map.iter().map(|(k, v)| (k.as_str().to_string(), *v)).collect()
}

pub fn run_estimate(args: EstimateArgs) -> Result<(), String> {
    if args.folders.is_empty() {
        return Err("No folders specified. Usage: nori estimate <folder>...".into());
    }

    let options = IndexOptions {
        code: args.is_code,
        ..Default::default()
    };

    let mut total_seconds = 0.0;

    for folder in &args.folders {
        let folder_str = folder.to_string_lossy().to_string();
        let norm_folder = folder_str.replace('\\', "/");

        if !folder.is_dir() {
            if args.json {
                let unreadable = json!({
                    "folder": norm_folder,
                    "readable": false,
                    "seconds": 0.0,
                    "files": HashMap::<String, usize>::new(),
                    "pdf_pages": 0,
                    "audio_seconds": 0.0,
                    "video_seconds": 0.0,
                });
                println!("{}", serde_json::to_string(&unreadable).unwrap());
            } else {
                eprintln!("{norm_folder}    can't read it (not a directory or not accessible)");
            }
            continue;
        }

        let started = Instant::now();
        let crawl = Crawler::crawl(std::slice::from_ref(folder), &options);
        let parts = Estimator::estimates_by_subfolder(&crawl.candidates, &norm_folder, &options, None);

        let estimate = parts
            .values()
            .cloned()
            .reduce(|a, b| a + b)
            .unwrap_or_default();

        let mut subfolders: Vec<(String, Estimate)> = parts
            .into_iter()
            .filter(|(k, _)| k != &norm_folder && k != &folder_str)
            .collect();

        // Sort descending by seconds, then ascending by path
        subfolders.sort_by(|a, b| {
            b.1.seconds
                .partial_cmp(&a.1.seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        total_seconds += estimate.seconds;

        if args.json {
            let subfolders_json: Vec<serde_json::Value> = subfolders
                .into_iter()
                .take(500)
                .map(|(sub_path, sub_est)| {
                    json!({
                        "folder": sub_path.replace('\\', "/"),
                        "seconds": sub_est.seconds,
                        "files": to_files_map(&sub_est.files),
                        "kind_seconds": to_kind_seconds_map(&sub_est.kind_seconds),
                        "long_files": sub_est.long_files,
                        "later_seconds": sub_est.later_seconds,
                        "bytes": sub_est.bytes,
                    })
                })
                .collect();

            let row = json!({
                "folder": norm_folder,
                "seconds": estimate.seconds,
                "readable": true,
                "files": to_files_map(&estimate.files),
                "kind_seconds": to_kind_seconds_map(&estimate.kind_seconds),
                "calibrated": true,
                "pdf_pages": estimate.pdf_pages,
                "pdf_pages_total": estimate.pdf_pages_total,
                "audio_seconds": estimate.audio_seconds,
                "video_seconds": estimate.video_seconds,
                "long_files": estimate.long_files,
                "later_seconds": estimate.later_seconds,
                "code_stretches": estimate.code_stretches,
                "bytes": estimate.bytes,
                "subfolders": subfolders_json,
                "skipped": crawl.skipped.values().sum::<usize>(),
                "datasets": crawl.datasets.len(),
                "ms": started.elapsed().as_millis() as u64,
            });

            // Single line JSON output per folder (read by Tauri helper)
            println!("{}", serde_json::to_string(&row).unwrap());
        } else {
            println!("\n📊 Crawl & Indexing Estimate: {}", norm_folder);
            println!("  Candidate files:       {}", crawl.candidates.len());
            println!("  Projected time:        {:.1}s", estimate.seconds);
            println!("  Estimated PDF pages:   {}", estimate.pdf_pages);
            println!("  Estimated doc chunks:  {}", estimate.doc_chunks);
            println!("  Estimated code stretches: {}", estimate.code_stretches);

            if !subfolders.is_empty() {
                println!("  Top subfolders:");
                for (sub, sub_est) in subfolders.iter().take(5) {
                    println!("    - {} ({:.1}s)", sub, sub_est.seconds);
                }
            }
        }
    }

    if !args.json && args.folders.len() > 1 {
        println!("\nTotal projected time: {:.1}s", total_seconds);
    }

    Ok(())
}
