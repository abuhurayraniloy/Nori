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

        Some("list") => {
            let conn = db::init_db().expect("failed to connect to db");
            let mut stmt = conn
                .prepare("SELECT id, file_path, COUNT(*) FROM document_chunks GROUP BY file_path")
                .expect("Query preparation failed");

            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .expect("Query Failed");

            for row in rows {
                let (id, path, count) = row.unwrap();
                println!("id:{} -> {} ({} chunks)", id, path, count);
            }
        }

        Some("delete") => {
            let id_str = match args.get(2) {
                Some(val) => val,
                None => {
                    eprintln!("Usage: cargo run -- delete <id>");
                    return;
                }
            };

            let id: i64 = match id_str.parse() {
                Ok(num) => num,
                Err(_) => {
                    eprintln!("Error: ID must be a valid number. Got: `{}`", id_str);
                    return;
                }
            };
            let conn = db::init_db().expect("failed to connect to the db");
            let rows_deleted = conn
                .execute(
                    "DELETE FROM document_chunks WHERE id = ?1",
                    rusqlite::params![id],
                )
                .expect("Failed to delete");

            if rows_deleted > 0 {
                println!("Deleted!");
            } else {
                println!("No chunks deleted")
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
