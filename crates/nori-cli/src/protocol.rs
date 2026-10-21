//! JSON-lines IPC protocol between the Desktop GUI Shell and Nori background services.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Timing measurements for search operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchTiming {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<f64>,
}

/// A hit returned by the search service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub score: f32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc_end: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_rank: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<HashMap<String, String>>,
}

/// Incoming request to `SearchService`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub op: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default = "default_true")]
    pub meaning: bool,
    #[serde(default)]
    pub prefix_last: bool,
    #[serde(default)]
    pub segment: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub loc: Option<f64>,
    #[serde(default)]
    pub pixels: Option<u32>,
}

fn default_true() -> bool {
    true
}

/// Outgoing response from `SearchService`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<Vec<SearchHit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meaning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<SearchTiming>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub searchable: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfinished: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Event emitted by `IndexService`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexEvent {
    pub event: String,
    #[serde(default)]
    pub code: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub searchable: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfinished: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<String>>,
}
