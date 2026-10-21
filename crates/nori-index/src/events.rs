use nori_core::types::FileKind;
use nori_crawl::CrawlReport;
use serde::{Deserialize, Serialize};

/// Summary of a sync operation reconciling crawler candidates with SQLite file records.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncSummary {
    pub added: usize,
    pub changed: usize,
    pub moved: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub crawl: CrawlReport,
    pub queued: Vec<i64>,
}

/// Notification emitted when a file has completed an indexing batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEvent {
    pub done: usize,
    pub total: usize,
    pub path: String,
    pub kind: FileKind,
    pub state: String,
    pub seconds: f64,
    pub note: Option<String>,
}

/// Notification emitted when a step of long file processing finishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepEvent {
    pub done: usize,
    pub total: usize,
    pub files: usize,
    pub path: String,
    pub kind: FileKind,
    pub state: String,
    pub seconds: f64,
    pub note: Option<String>,
}
