use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

pub fn wait_until_file_ready(path: &Path) -> bool {
    // Ignore temp or incomplete file
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext = ext.to_lowercase();
        if ext == "crdownload" || ext == "part" || ext == "tmp" || ext == "download" {
            return false;
        }
    }

    // Poll the file to ensure the size has stopped changing
    let mut last_size = 0;
    let mut stable_count = 0;

    for i in 0..10 {
        println!("Iteration: {i}");
        // Try for up to ~5 seconds
        if let Ok(metadata) = path.metadata() {
            let current_size = metadata.len();
            println!("current_size: {current_size}");
            // we can only care about files with content (>0 bytes)
            if current_size > 0 && current_size == last_size {
                stable_count += 1;
                println!("stable_count: {}", stable_count);
                // If size remained identical for 2 consecutive checks, verify file lock
                if stable_count > 1 && can_open_exclusively(path) {
                    return true;
                }
            } else {
                stable_count = 0;
                last_size = current_size;
            }
        }
        sleep(Duration::from_millis(500));
    }
    false
}

// Checks if the browser or os has released it's write lock
fn can_open_exclusively(path: &Path) -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .is_ok()
}
