//! Command: nori status

use nori_storage::store::IndexStore;
use std::path::Path;

pub struct StatusArgs {
    pub is_code: bool,
    pub db_path: Option<String>,
    pub json: bool,
}

pub fn run_status(args: StatusArgs) -> Result<(), String> {
    let default_db = if args.is_code {
        "nori_code.sqlite"
    } else {
        "nori.sqlite"
    };
    let db_path = args.db_path.as_deref().unwrap_or(default_db);

    if !Path::new(db_path).exists() {
        if args.json {
            println!("{}", serde_json::json!({ "exists": false, "database": db_path }));
        } else {
            println!("ℹ️ Database '{}' does not exist yet.", db_path);
        }
        return Ok(());
    }

    let store = IndexStore::open(Path::new(db_path))
        .map_err(|e| format!("failed to open database '{db_path}': {e}"))?;

    let raw = store.db().raw();

    let file_count: i64 = raw
        .query_row("SELECT count(*) FROM files;", [], |r| r.get(0))
        .unwrap_or(0);

    let segment_count: i64 = raw
        .query_row("SELECT count(*) FROM segments;", [], |r| r.get(0))
        .unwrap_or(0);

    let vector_count: i64 = raw
        .query_row(
            "SELECT count(*) FROM segments WHERE vec IS NOT NULL;",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let db_size_bytes = std::fs::metadata(db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    if args.json {
        let json_val = serde_json::json!({
            "database": db_path,
            "exists": true,
            "is_code": args.is_code,
            "files": file_count,
            "segments": segment_count,
            "vectors": vector_count,
            "database_size_bytes": db_size_bytes,
        });
        println!("{}", serde_json::to_string_pretty(&json_val).unwrap());
    } else {
        println!("\n📦 Nori Index Status ({}):", db_path);
        println!("  Files tracked:     {}", file_count);
        println!("  Text segments:     {}", segment_count);
        println!("  Embedding vectors: {}", vector_count);
        println!("  Database size:     {:.2} MB\n", db_size_bytes as f64 / (1024.0 * 1024.0));
    }

    Ok(())
}
