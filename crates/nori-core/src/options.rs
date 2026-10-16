use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Indexing guardrails. Anything that changes vectors is part of `fingerprint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexOptions {
    /// True if this index is dedicated strictly to source code repositories.
    pub code: bool,
    /// Maximum characters in a code stretch (default: 1,800 chars / ~450 tokens).
    pub code_stretch: usize,
    /// Maximum byte size for a code file (default: 1 MB = 1 << 20). Larger files are machine-generated.
    pub max_code_bytes: usize,
    /// Maximum byte size for a Jupyter notebook (default: 20 MB = 20 << 20).
    pub max_notebook_bytes: usize,
    pub include_code: bool,
    pub include_datasets: bool,
    /// Minimum dimension of an image (default: 256px). Images smaller than this are skipped.
    pub min_image_side: u32,
    /// PDFs up to this page count are indexed in a single pass (default: 50).
    pub big_pdf_pages: usize,
    /// Page step size for incremental multi-pass PDF indexing (default: 30 pages).
    pub pdf_step: usize,
    /// Chunk step size for incremental multi-pass document indexing (default: 40 chunks).
    pub doc_step: usize,
    /// Safety ceiling for giant machine-generated PDFs (default: 5,000 pages).
    pub max_pages: usize,
    /// Safety ceiling for giant text documents (default: 5,000 chunks / ~8M characters).
    pub max_chunks: usize,
    /// Character threshold below which a PDF page is treated as a scan/slide and rendered as an image (default: 200).
    pub min_page_text: usize,
    /// Vision soft tokens for general photos: 70 / 140 / 280 / 560 / 1120 (default: 140).
    pub photo_budget: usize,
    /// Vision soft tokens for screenshots (default: 280).
    pub screenshot_budget: usize,
    /// Vision soft tokens for rendered PDF pages (default: 280).
    pub page_budget: usize,
    /// Vision soft tokens for video keyframes (default: 70).
    pub frame_budget: usize,
    /// Sampling interval in seconds between candidate video frames (default: 3.0s).
    pub frame_interval: f64,
    /// Audio window size in seconds (default: 30.0s). EmbeddingGemma ignores audio beyond 30s.
    pub audio_window: f64,
    /// Audio sliding window hop in seconds (default: 25.0s, giving 5.0s overlap).
    pub audio_hop: f64,
    pub excluded_folders: Vec<String>,
    pub excluded_extensions: HashSet<String>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            code: false,
            code_stretch: 1800,
            max_code_bytes: 1 << 20,
            max_notebook_bytes: 20 << 20,
            include_code: false,
            include_datasets: false,
            min_image_side: 256,
            big_pdf_pages: 50,
            pdf_step: 30,
            doc_step: 40,
            max_pages: 5000,
            max_chunks: 5000,
            min_page_text: 200,
            photo_budget: 140,
            screenshot_budget: 280,
            page_budget: 280,
            frame_budget: 70,
            frame_interval: 3.0,
            audio_window: 30.0,
            audio_hop: 25.0,
            excluded_folders: Vec::new(),
            excluded_extensions: HashSet::new(),
        }
    }
}

impl IndexOptions {
    /// Parses human-entered extension lists (e.g. `"HEIC, .txt *.md"`) into normalized lowercase sets `["heic", "txt", "md"]`.
    pub fn parse_extensions(text: &str) -> HashSet<String> {
        text.split(|c: char| c == ',' || c.is_whitespace())
            .map(|word| word.trim_start_matches(['*', '.']).to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Generate fingerprint representing vector compatibility.
    pub fn fingerprint(&self) -> String {
        if self.code {
            format!("code1 lines{}", self.code_stretch)
        } else {
            format!(
                "photo{} shot{} page{} frame{}/{}s audio{}/{}s",
                self.photo_budget,
                self.screenshot_budget,
                self.page_budget,
                self.frame_budget,
                self.frame_interval,
                self.audio_window,
                self.audio_hop
            )
        }
    }
}
