mod db;
mod debounce;
// [CHANGE 1] Suppress unused function warnings in extractors while sorting purely by extension
#[allow(dead_code)]
mod extractors;
// [CHANGE 2] Register the new media module
mod indexer;
mod media;
mod watcher;

use std::env;
// [CHANGE 3] Removed unused `Path` from import
use std::path::PathBuf;

fn main() {
    println!("🚀 Starting Nori initialization...");

    if let Err(e) = db::init_db() {
        eprintln!("❌ Failed to initialize database: {}", e);
        return;
    }
    println!("✅ Database initialized successfully.");

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
