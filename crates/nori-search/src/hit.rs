use nori_core::types::{FileKind, SegmentKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A ranked search hit produced by hybrid semantic + keyword fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Full path of the file on disk.
    pub path: String,
    /// Categorized file type.
    pub kind: FileKind,
    /// Fused rank score (for ordering only).
    pub score: f64,
    /// How far the best segment stands out for this query (z-score, or None if keyword-only).
    pub z: Option<f64>,
    /// Raw cosine similarity of the best segment (or None if keyword-only).
    pub cosine: Option<f64>,
    /// 1-based rank among keyword matches (or None if semantic-only).
    pub keyword_rank: Option<usize>,
    /// Segment type pointed at by this hit.
    pub segment: SegmentKind,
    /// Database ID of the matched segment.
    pub segment_id: i64,
    /// Page number (for PDF) or start seconds (for audio/video).
    pub loc: Option<f64>,
    /// End boundary or duration in seconds.
    pub loc_end: Option<f64>,
    /// Matched snippet or stored segment excerpt with highlight brackets.
    pub excerpt: Option<String>,
    /// File modification timestamp (seconds since unix epoch).
    pub modified: f64,
    /// File size in bytes.
    pub size: i64,
    /// Media metadata (e.g., width, height, pages, duration).
    pub info: HashMap<String, String>,
    /// Code line to open at (where identifier is defined or query words match).
    pub focus: Option<f64>,
}
