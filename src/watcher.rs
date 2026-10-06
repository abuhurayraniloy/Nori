use crate::debounce::wait_until_file_ready;
// [CHANGE 1] Import sorting logic instead of heavy text extractors
use crate::media::sort_file_by_extension;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::channel};
use std::thread;

pub fn start_watching<P: AsRef<Path>>(watch_path: P) -> notify::Result<()> {
    let watch_dir = watch_path.as_ref();
    // [CHANGE 2] Define the destination directory under watch_dir/Organized
    let output_dir = watch_dir.join("Organized");

    // 1. Create communication channel (Receiver and Sender)
    // The OS listener sends events into `tx` and our loop reads them from `rx`
    let (tx, rx) = channel();

    // 2. Initialize the windows filesystem watcher
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    // 3. Tell windows which folder to monitor
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    println!("👀 Watching directory: {:?}", watch_dir);
    println!("📁 Output directory: {:?}", output_dir);
    println!("👉 Try downloading or dropping a file there now...\n");

    // Track paths to avoid processing duplicate events
    let processed = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

    // 4. Infinite event listening loop
    for res in rx {
        match res {
            // [CHANGE 3] Pass output_dir to the event handler
            Ok(event) => handle_event(event, &output_dir, &processed),
            Err(e) => eprintln!("❌ Watcher error: {:?}", e),
        }
    }

    Ok(())
}

fn handle_event(event: Event, output_dir: &Path, processed: &Arc<Mutex<HashSet<PathBuf>>>) {
    // We only care when file is created or modified.
    match event.kind {
        EventKind::Remove(_) => {
            for path in event.paths {
                processed.lock().unwrap().remove(&path);
            }
        }

        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in event.paths {
                // [CHANGE 4] Ignore files already in Organized, directories, or already-processed files
                if path.starts_with(output_dir)
                    || !path.is_file()
                    || !processed.lock().unwrap().insert(path.clone())
                {
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
                let output_dir = output_dir.to_path_buf();

                // Check if file is ready and unlocked
                thread::spawn(move || {
                    if wait_until_file_ready(&path) {
                        println!("📂 File detected: {:?}", path);

                        // [CHANGE 5] Move file to its extension folder instead of running OCR/PDF extraction
                        match sort_file_by_extension(&path, &output_dir) {
                            Ok(dest) => {
                                println!(
                                    "📦 Organized: {:?} -> {:?}",
                                    path.file_name().unwrap_or_default(),
                                    dest
                                );
                            }
                            Err(e) => {
                                eprintln!("❌ Failed to organize {:?}: {}", path, e);
                                processed.lock().unwrap().remove(&path);
                            }
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
