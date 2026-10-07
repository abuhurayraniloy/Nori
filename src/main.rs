mod db;
mod debounce;
mod extractors;
mod indexer;
mod media;
mod watcher;

use std::env;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = env::args().collect();

    println!("DEBUG args = {:?}", args);

    println!("🚀 Starting Nori initialization...");

    if let Err(e) = db::init_db() {
        eprintln!("❌ Failed to initialize database: {}", e);
        return;
    }
    println!("✅ Database initialized successfully.");

    let command = args.get(1).map(|s| s.to_lowercase());

    match command.as_deref() {
        Some("search") => {
            let query = match args.get(2..) {
                Some(words) if !words.is_empty() => words.join(" "),
                _ => {
                    eprintln!("Usage: cargo run --search <your query>");
                    return;
                }
            };

            println!("🔍 Searching for: \"{}\"...", query);

            let conn = db::init_db().expect("Failed to connect to db");
            let embedder = indexer::Embedder::new().expect("Failed to initialize embedder");

            match embedder.search(&conn, &query, 5) {
                Ok(results) if results.is_empty() => {
                    println!("No matching documents found");
                }

                Ok(results) => {
                    println!("\nSearch results\n");
                    for (i, res) in results.iter().enumerate() {
                        println!("\n🏆 #{}: Match Score: {:.1}%", i + 1, res.score * 100.0);
                        println!("📁 File: {}", res.file_path);
                        println!("📝 Preview:\n{}", res.chunk_text.trim());
                    }
                }

                Err(e) => eprintln!("X Search failed: {e}"),
            }
        }

        Some("watch") | None => {
            println!("🚀 Starting Nori file watcher...");

            // Automatically locate the user's Downloads directory on windows
            let user_profile =
                env::var("USERPROFILE").expect("Could not find USERPROFILE environment variable");
            let downloads_dir = PathBuf::from(user_profile).join("Downloads");

            if !downloads_dir.exists() {
                eprintln!("❌ Downloads directory not found at: {:?}", downloads_dir);
                return;
            }

            // Start the watcher
            if let Err(e) = watcher::start_watching(&downloads_dir) {
                eprintln!("❌ Failed to start watcher: {:?}", e);
            }
        }

        Some(other) => {
            eprintln!("Unknown command: \"{}\"", other);
            println!("Available commands:");
            println!("  cargo run -- watch");
            println!("  cargo run -- search <query>");
        }
    }
}
