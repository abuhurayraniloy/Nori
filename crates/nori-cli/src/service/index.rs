//! IndexService: Out-of-process indexing worker communicating over stdin/stdout.

use crate::protocol::IndexEvent;
use crate::service::watchdog::Watchdog;
use nori_core::options::IndexOptions;
use nori_index::Indexer;
use nori_infer::{EmbeddingWorker, MockEmbedder, WorkerConfig};
use nori_storage::store::IndexStore;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct IndexService {
    db_path: PathBuf,
    roots: Vec<PathBuf>,
    code_roots: Vec<PathBuf>,
    is_code: bool,
    backfill_allowed: bool,
    running: Arc<AtomicBool>,
}

impl IndexService {
    pub fn new(db_path: PathBuf, roots: Vec<PathBuf>, code_roots: Vec<PathBuf>, is_code: bool) -> Self {
        Self {
            db_path,
            roots,
            code_roots,
            is_code,
            backfill_allowed: true,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Run the indexing service command loop reading from stdin and writing events to stdout.
    pub fn run_loop<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> std::io::Result<()> {
        let emit = |w: &mut W, ev: IndexEvent| -> std::io::Result<()> {
            if let Ok(json) = serde_json::to_string(&ev) {
                writeln!(w, "{}", json)?;
                w.flush()?;
            }
            Ok(())
        };

        // Start watchdog
        let index_dir = self.db_path.parent().unwrap_or_else(|| Path::new("."));
        let watchdog = Watchdog::new(index_dir, None, None);
        let current_file = Arc::new(std::sync::Mutex::new(None));
        watchdog.start(Arc::clone(&current_file));

        emit(
            &mut writer,
            IndexEvent {
                event: "sync".into(),
                code: self.is_code,
                message: Some("index-service started".into()),
                roots: Some(
                    self.roots
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                ),
                ..Default::default()
            },
        )?;

        // Do initial sync
        self.perform_sync(&mut writer, false, &current_file)?;

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match trimmed {
                "sync" => {
                    self.perform_sync(&mut writer, false, &current_file)?;
                }
                "sync code" => {
                    self.perform_sync(&mut writer, true, &current_file)?;
                }
                "backfill on" => {
                    self.backfill_allowed = true;
                }
                "backfill off" => {
                    self.backfill_allowed = false;
                }
                "stop" | "quit" | "exit" => {
                    self.running.store(false, Ordering::SeqCst);
                    break;
                }
                other => {
                    emit(
                        &mut writer,
                        IndexEvent {
                            event: "error".into(),
                            code: self.is_code,
                            message: Some(format!("unknown index-service command: '{other}'")),
                            ..Default::default()
                        },
                    )?;
                }
            }
        }

        watchdog.stop();
        emit(
            &mut writer,
            IndexEvent {
                event: "exit".into(),
                code: self.is_code,
                message: Some("index-service exited cleanly".into()),
                ..Default::default()
            },
        )?;

        Ok(())
    }

    fn perform_sync<W: Write>(
        &mut self,
        writer: &mut W,
        code_pass: bool,
        current_file: &Arc<std::sync::Mutex<Option<(String, Instant)>>>,
    ) -> std::io::Result<()> {
        let emit = |w: &mut W, ev: IndexEvent| -> std::io::Result<()> {
            if let Ok(json) = serde_json::to_string(&ev) {
                writeln!(w, "{}", json)?;
                w.flush()?;
            }
            Ok(())
        };

        let target_roots = if code_pass {
            &self.code_roots
        } else {
            &self.roots
        };

        if target_roots.is_empty() {
            return emit(
                writer,
                IndexEvent {
                    event: "idle".into(),
                    code: code_pass,
                    message: Some("no roots configured for sync".into()),
                    ..Default::default()
                },
            );
        }

        let store = match IndexStore::open(&self.db_path) {
            Ok(s) => s,
            Err(e) => {
                return emit(
                    writer,
                    IndexEvent {
                        event: "error".into(),
                        code: code_pass,
                        message: Some(format!("failed to open DB: {e}")),
                        ..Default::default()
                    },
                );
            }
        };

        let embedder: Box<dyn nori_infer::traits::Embedder> =
            if let Some(Ok(config)) = WorkerConfig::from_environment() {
                if let Ok(worker) = EmbeddingWorker::new(config, true) {
                    Box::new(worker)
                } else {
                    Box::new(MockEmbedder::new())
                }
            } else {
                Box::new(MockEmbedder::new())
            };

        let options = IndexOptions {
            code: code_pass || self.is_code,
            ..Default::default()
        };

        let mut indexer = match Indexer::new(store, options, embedder) {
            Ok(idx) => idx,
            Err(e) => {
                return emit(
                    writer,
                    IndexEvent {
                        event: "error".into(),
                        code: code_pass,
                        message: Some(format!("failed to initialize indexer: {e}")),
                        ..Default::default()
                    },
                );
            }
        };

        let sync_summary = match indexer.sync(target_roots) {
            Ok(s) => s,
            Err(e) => {
                return emit(
                    writer,
                    IndexEvent {
                        event: "error".into(),
                        code: code_pass,
                        message: Some(format!("sync crawl error: {e}")),
                        ..Default::default()
                    },
                );
            }
        };

        let total = sync_summary.queued.len();
        let counts = indexer.store.counts().unwrap_or_default();
        emit(
            writer,
            IndexEvent {
                event: "sync".into(),
                code: code_pass,
                path: None,
                message: Some(format!(
                    "sync queued {} files ({} added, {} changed)",
                    total, sync_summary.added, sync_summary.changed
                )),
                indexed: Some(0),
                done: Some(0),
                total: Some(total),
                new: Some(total),
                files: Some(counts.files),
                searchable: Some(counts.searchable),
                pending: Some(counts.pending),
                unfinished: Some(counts.unfinished),
                roots: Some(
                    target_roots
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                ),
                ..Default::default()
            },
        )?;

        let running = Arc::clone(&self.running);
        let curr_file_for_run = Arc::clone(current_file);

        let states = indexer.run(
            None,
            Some(&sync_summary.queued),
            move || !running.load(Ordering::Relaxed),
            |event| {
                if let Ok(mut guard) = curr_file_for_run.lock() {
                    *guard = Some((event.path.clone(), Instant::now()));
                }

                let _ = emit(
                    writer,
                    IndexEvent {
                        event: "file".into(),
                        code: code_pass,
                        path: Some(event.path),
                        kind: Some(event.kind.as_str().to_string()),
                        message: None,
                        indexed: Some(event.done),
                        done: Some(event.done),
                        total: Some(event.total),
                        new: Some(event.total),
                        state: Some(event.state),
                        ..Default::default()
                    },
                );
            },
        );

        if let Ok(mut guard) = current_file.lock() {
            *guard = None;
        }

        let indexed_count: usize = states.map(|s| s.values().sum()).unwrap_or(0);

        // Finish long files if backfill is allowed
        if self.backfill_allowed {
            let _ = indexer.finish_long_files(|| false, |_| {});
        }

        let final_counts = indexer.store.counts().unwrap_or_default();

        emit(
            writer,
            IndexEvent {
                event: "idle".into(),
                code: code_pass,
                path: None,
                message: Some(format!("sync complete, {indexed_count} files processed")),
                indexed: Some(indexed_count),
                done: Some(indexed_count),
                total: Some(total),
                new: Some(0),
                files: Some(final_counts.files),
                searchable: Some(final_counts.searchable),
                pending: Some(final_counts.pending),
                unfinished: Some(final_counts.unfinished),
                ..Default::default()
            },
        )?;

        Ok(())
    }
}
