use crate::debounce::wait_until_file_ready;
use crate::extractors::extract_content;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::channel};
use std::thread;

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
    let processed = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

    // 4. Infinite event listening loop
    for res in rx {
        match res {
            Ok(event) => handle_event(event, &processed),
            Err(e) => eprintln!("❌ Watcher error: {:?}", e),
        }
    }

    Ok(())
}

fn handle_event(event: Event, processed: &Arc<Mutex<HashSet<PathBuf>>>) {
    // We only care when file is created or modified.
    match event.kind {
        EventKind::Remove(_) => {
            for path in event.paths {
                processed.lock().unwrap().remove(&path);
            }
        }

        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in event.paths {
                // Ignore already processed or temp folders
                if !path.is_file() || !processed.lock().unwrap().insert(path.clone()) {
                    continue;
                }

                // Ignore temp or incomplete file
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let ext = ext.to_lowercase();
                    if ext == "crdownload" || ext == "part" || ext == "tmp" || ext == "download" {
                        continue;
                    }
                }

                let processed = Arc::clone(processed);

                // Check if file is ready and unlocked
                thread::spawn(move || {
                    if wait_until_file_ready(&path) {
                        println!("📂 File detected: {:?}", path);
                        println!("Ready for extraction and renaming!");

                        let document = extract_content(&path);

                        match document {
                            Ok(doc) => {
                                println!("Document source: {:?}", doc.source_path);
                                println!("Text: \n {}", doc.text);
                                println!("Is truncated: {}", doc.truncated);
                            }
                            Err(e) => eprintln!("Could not extract the document, error {:?}", e),
                        }
                    } else {
                        // Allow later create, modify event to retry
                        processed.lock().unwrap().remove(&path);
                    }
                });
            }
        }
        _ => {} // Ignore the other events
    }
}
