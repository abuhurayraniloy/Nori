use crate::debounce::wait_until_file_ready;

use std::collections::HashSet;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

pub fn start_watching<P: AsRef<Path>>(watch_path: P) -> notify::Result<()> {
    // 1. Create communication channel (Receiver and Sender)
    // The OS listener sends events into `tx` and our loop reads them from `rx`
    let (tx, rx) = channel();

    // 2. Initialize the windows filessystem watcher
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    // 3. Tell windows which folder to monitor

    watcher.watch(watch_path.as_ref(), RecursiveMode::NonRecursive)?;

    println!("👀 Watching directory: {:?}", watch_path.as_ref());
    println!("👉 Try downloading or dropping a file there now...\n");

    // Track paths to avoid processing duplicate events
    let mut processed: HashSet<PathBuf> = HashSet::new();

    // 4. Infinite event listening loop
    for res in rx {
        match res {
            Ok(event) => handle_event(event, &mut processed),
            Err(e) => eprintln!("❌ Watcher error: {:?}", e),
        }
    }

    Ok(())
}

fn handle_event(event: Event, processed: &mut HashSet<PathBuf>) {
    // We only care when file is created or modified.
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in event.paths {
                // Ignore already processed or temp folders
                if processed.contains(&path) || !path.is_file() {
                    continue;
                }

                // Check if file is ready and unlocked
                if wait_until_file_ready(&path) {
                    processed.insert(path.clone());
                    println!("📂 File detected: {:?}", path);
                    println!("Ready for extraction and renaming!");
                }
            }
        }
        _ => {} // Ignore the other events
    }
}
