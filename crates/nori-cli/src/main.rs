//! nori: offline multimodal semantic search engine CLI

use nori_cli::commands::{
    run_download_model, run_estimate, run_index, run_search, run_speed, run_status,
    DownloadModelArgs, EstimateArgs, IndexArgs, SearchArgs, SpeedArgs, StatusArgs,
};
use nori_cli::service::{IndexService, SearchService, WorkerService};
use std::env;
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;

fn print_usage() {
    println!(
        r#"nori: offline multimodal semantic search engine

Usage:
  nori search <words>... [--limit N] [--code] [--json] [--kind K,K]
      Hybrid meaning (vectors) + keyword (FTS5) search across indexed assets.
  nori index <folder>... [--code] [--limit N]
      Synchronize database catalog, extract media, and embed pending assets.
  nori estimate <folder>... [--code] [--json]
      Analyze file counts, pages, and projected indexing time without writing.
  nori status [--code] [--json]
      Display index health, file counts, segments, and database size.
  nori speed [--tries N] [--json]
      Benchmark machine SIMD performance and sample CPU load.
  nori download-model [--to DIR]
      Download and verify EmbeddingGemma 2 GGUF models.
  nori search-service [--code] [--index DIR]
      Start background SearchService daemon (JSON-lines on stdin/stdout).
  nori index-service <folder>... [--code-root DIR]... [--code] [--index DIR]
      Start background IndexService worker daemon (JSON-lines on stdin/stdout).
  nori worker [--text-only]
      Start background embedding worker daemon (JSON-lines on stdin/stdout).
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    let command = args[1].to_lowercase();
    if command == "--help" || command == "-h" || command == "help" {
        print_usage();
        return;
    }
    let is_code = args.iter().any(|a| a == "--code");
    let json = args.iter().any(|a| a == "--json");

    let get_opt = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };

    let get_all_opts = |name: &str| -> Vec<String> {
        let mut res = Vec::new();
        let mut idx = 0;
        while idx < args.len() {
            if args[idx] == name && idx + 1 < args.len() {
                res.push(args[idx + 1].clone());
                idx += 2;
            } else {
                idx += 1;
            }
        }
        res
    };

    match command.as_str() {
        "search" => {
            let words: Vec<String> = args[2..]
                .iter()
                .filter(|a| !a.starts_with("--") && !a.starts_with("-"))
                .cloned()
                .collect();

            let limit = get_opt("--limit")
                .or_else(|| get_opt("-n"))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(10);

            let kinds = get_opt("--kind")
                .map(|k| k.split(',').map(|s| s.trim().to_string()).collect());

            let db_path = get_opt("--index");

            if let Err(e) = run_search(SearchArgs {
                words,
                limit,
                is_code,
                json,
                kinds,
                db_path,
            }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        "index" => {
            let folders: Vec<PathBuf> = args[2..]
                .iter()
                .filter(|a| !a.starts_with("--") && !a.starts_with("-"))
                .map(PathBuf::from)
                .collect();

            let limit = get_opt("--limit").and_then(|s| s.parse::<usize>().ok());
            let db_path = get_opt("--index");

            if let Err(e) = run_index(IndexArgs {
                folders,
                is_code,
                db_path,
                limit,
            }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        "estimate" => {
            let folders: Vec<PathBuf> = args[2..]
                .iter()
                .filter(|a| !a.starts_with("--") && !a.starts_with("-"))
                .map(PathBuf::from)
                .collect();

            if let Err(e) = run_estimate(EstimateArgs {
                folders,
                is_code,
                json,
            }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        "status" => {
            let db_path = get_opt("--index");
            if let Err(e) = run_status(StatusArgs {
                is_code,
                db_path,
                json,
            }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        "speed" => {
            let tries = get_opt("--tries")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(5);
            if let Err(e) = run_speed(SpeedArgs { tries, json }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        "search-service" => {
            let db_path = resolve_db_path(get_opt("--index"), is_code);

            let mut service = match SearchService::new(&db_path, is_code) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("❌ Failed to initialize SearchService: {e}");
                    std::process::exit(1);
                }
            };

            let stdin_handle = stdin();
            let stdout_handle = stdout();
            if let Err(e) = service.run_loop(BufReader::new(stdin_handle.lock()), stdout_handle.lock()) {
                eprintln!("❌ SearchService loop error: {e}");
            }
        }
        "index-service" => {
            let folders: Vec<PathBuf> = args[2..]
                .iter()
                .filter(|a| !a.starts_with("--") && !a.starts_with("-"))
                .map(PathBuf::from)
                .collect();

            let code_roots: Vec<PathBuf> = get_all_opts("--code-root")
                .into_iter()
                .map(PathBuf::from)
                .collect();

            let db_path = resolve_db_path(get_opt("--index"), is_code);

            let mut service = IndexService::new(db_path, folders, code_roots, is_code);
            let stdin_handle = stdin();
            let stdout_handle = stdout();
            if let Err(e) = service.run_loop(BufReader::new(stdin_handle.lock()), stdout_handle.lock()) {
                eprintln!("❌ IndexService loop error: {e}");
            }
        }
        "worker" => {
            let text_only = args.iter().any(|a| a == "--text-only");
            let mut service = WorkerService::new(text_only);
            let stdin_handle = stdin();
            let stdout_handle = stdout();
            if let Err(e) = service.run(BufReader::new(stdin_handle.lock()), stdout_handle.lock()) {
                eprintln!("❌ WorkerService loop error: {e}");
            }
        }
        "download-model" => {
            let to = get_opt("--to").map(PathBuf::from);
            if let Err(e) = run_download_model(DownloadModelArgs { to }) {
                eprintln!("❌ {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Unknown command: '{}'", command);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn resolve_db_path(raw: Option<String>, is_code: bool) -> PathBuf {
    let db_filename = if is_code { "nori_code.sqlite" } else { "nori.sqlite" };
    let path = match raw {
        Some(path_str) => {
            let p = PathBuf::from(path_str);
            if p.is_dir() || p.extension().is_none() {
                p.join(db_filename)
            } else {
                p
            }
        }
        None => {
            let local_db = PathBuf::from(db_filename);
            if local_db.exists() {
                local_db
            } else {
                let dir = nori_core::media_files::AppPaths::default_index();
                let _ = std::fs::create_dir_all(&dir);
                dir.join(db_filename)
            }
        }
    };

    if path.is_relative() {
        if let Ok(cur) = std::env::current_dir() {
            return cur.join(path);
        }
    }
    path
}
