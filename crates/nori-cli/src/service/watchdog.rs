//! Memory and execution time watchdog for indexing processes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Exit code emitted when watchdog terminates the process on an unreadable file.
pub const WATCHDOG_EXIT_CODE: i32 = 85;

/// Information about a file that caused the indexing process to abort.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoppedFile {
    pub path: Option<String>,
    pub why: String,
    pub timestamp_ms: i64,
}

impl StoppedFile {
    pub fn note_path(folder: &Path) -> PathBuf {
        folder.join("stopped_file.json")
    }

    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }

    pub fn read_from(path: &Path) -> Option<Self> {
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}

pub struct Watchdog {
    memory_limit: u64,
    time_limit: Duration,
    note: PathBuf,
    running: Arc<AtomicBool>,
}

impl Watchdog {
    /// Default memory limit: 4 GB.
    pub const DEFAULT_MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
    /// Default single-file timeout: 15 minutes.
    pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(15 * 60);

    pub fn new(index_dir: &Path, memory_limit: Option<u64>, time_limit: Option<Duration>) -> Self {
        Self {
            memory_limit: memory_limit.unwrap_or(Self::DEFAULT_MEMORY_LIMIT),
            time_limit: time_limit.unwrap_or(Self::DEFAULT_TIME_LIMIT),
            note: StoppedFile::note_path(index_dir),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Query the current process memory footprint (bytes).
    pub fn current_memory_footprint() -> Option<u64> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::ProcessStatus::{
                K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
            };
            use windows_sys::Win32::System::Threading::GetCurrentProcess;

            unsafe {
                let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                if K32GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    &mut pmc,
                    std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ) != 0
                {
                    Some(pmc.WorkingSetSize as u64)
                } else {
                    None
                }
            }
        }

        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Start the watchdog monitor thread.
    pub fn start(&self, current_file: Arc<std::sync::Mutex<Option<(String, Instant)>>>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let running = Arc::clone(&self.running);
        let mem_limit = self.memory_limit;
        let time_limit = self.time_limit;
        let note = self.note.clone();

        thread::Builder::new()
            .name("nori-watchdog".into())
            .spawn(move || {
                while running.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(1));

                    if let Some(footprint) = Self::current_memory_footprint() {
                        if footprint > mem_limit {
                            let (path, _) = current_file
                                .lock()
                                .ok()
                                .and_then(|g| g.clone())
                                .unwrap_or((String::from("<unknown>"), Instant::now()));

                            let stopped = StoppedFile {
                                path: Some(path.clone()),
                                why: format!("Exceeded memory limit of {} MB", mem_limit >> 20),
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };
                            let _ = stopped.write_to(&note);
                            eprintln!("🚨 Watchdog: process killed, {} exceeded memory limit", path);
                            std::process::exit(WATCHDOG_EXIT_CODE);
                        }
                    }

                    if let Ok(guard) = current_file.lock() {
                        if let Some((path, started)) = &*guard {
                            if started.elapsed() > time_limit {
                                let stopped = StoppedFile {
                                    path: Some(path.clone()),
                                    why: format!("Exceeded time limit of {:?}", time_limit),
                                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                                };
                                let _ = stopped.write_to(&note);
                                eprintln!("🚨 Watchdog: process killed, {} exceeded time limit", path);
                                std::process::exit(WATCHDOG_EXIT_CODE);
                            }
                        }
                    }
                }
            })
            .expect("failed to spawn watchdog thread");
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
