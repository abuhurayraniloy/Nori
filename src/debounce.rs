use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_POLLS: usize = 10;
const REQUIRED_STABLE_CHECKS: usize = 3;

pub fn wait_until_file_ready(path: &Path) -> bool {
    wait_until_file_ready_with(path, POLL_INTERVAL, MAX_POLLS)
}

fn wait_until_file_ready_with(path: &Path, interval: Duration, max_polls: usize) -> bool {
    // Poll the file to ensure the size has stopped changing
    let mut last_size = None;
    let mut stable_count = 0;

    for i in 0..max_polls {
        println!("Iteration: {i}");
        // Try for up to ~5 seconds
        if let Ok(metadata) = path.metadata() {
            let current_size = metadata.len();
            println!("current_size: {current_size}");

            if observe_size(&mut last_size, &mut stable_count, current_size)
                && can_open_exclusively(path)
            {
                return true;
            }

            println!("stable_count: {stable_count}");
        } else {
            last_size = None;
            stable_count = 0;
        }

        sleep(interval);
    }

    false
}

fn observe_size(last_size: &mut Option<u64>, stable_count: &mut usize, current_size: u64) -> bool {
    if current_size == 0 {
        *last_size = None;
        *stable_count = 0;
        return false;
    }

    if *last_size == Some(current_size) {
        *stable_count += 1;
    } else {
        *last_size = Some(current_size);
        *stable_count = 0;
    }

    *stable_count >= REQUIRED_STABLE_CHECKS
}

// Checks if the browser or os has released it's write lock
fn can_open_exclusively(path: &Path) -> bool {
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nori-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn stable_size_requires_three_unchanged_checks() {
        let mut last_size = None;
        let mut stable_count = 0;

        assert!(!observe_size(&mut last_size, &mut stable_count, 100));
        assert!(!observe_size(&mut last_size, &mut stable_count, 100));
        assert!(!observe_size(&mut last_size, &mut stable_count, 100));
        assert!(observe_size(&mut last_size, &mut stable_count, 100));
    }

    #[test]
    fn changing_size_resets_stability() {
        let mut last_size = None;
        let mut stable_count = 0;

        observe_size(&mut last_size, &mut stable_count, 100);
        observe_size(&mut last_size, &mut stable_count, 100);
        assert_eq!(stable_count, 1);

        assert!(!observe_size(&mut last_size, &mut stable_count, 200));
        assert_eq!(last_size, Some(200));
        assert_eq!(stable_count, 0);
    }

    #[test]
    fn zero_size_resets_stability() {
        let mut last_size = Some(100);
        let mut stable_count = 2;

        assert!(!observe_size(&mut last_size, &mut stable_count, 0));
        assert_eq!(last_size, None);
        assert_eq!(stable_count, 0);
    }

    #[test]
    fn missing_file_is_not_ready() {
        let path = temporary_path("missing");

        assert!(!wait_until_file_ready_with(&path, Duration::ZERO, 4));
    }

    #[test]
    fn stable_nonempty_file_is_ready() {
        let path = temporary_path("stable");
        fs::write(&path, b"ready").unwrap();

        assert!(wait_until_file_ready_with(&path, Duration::ZERO, 4));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn exclusively_locked_file_is_not_ready() {
        let path = temporary_path("locked");
        fs::write(&path, b"locked").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();

        assert!(!wait_until_file_ready_with(&path, Duration::ZERO, 4));

        drop(lock);
        fs::remove_file(path).unwrap();
    }
}
