use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Screenshot,
    Image,
    Pdf,
    Doc,
    Audio,
    Video,
    Code,
}

impl FileKind {
    /// The order for first indexing pass: what people look for most pass.
    /// Priority: Screenshot (0) > Image (1) > Pdf (2) > Doc (3) > Audio (4) > Video (5) > Code (6).
    #[inline]
    pub const fn priority(&self) -> u8 {
        match self {
            FileKind::Screenshot => 0,
            FileKind::Image => 1,
            FileKind::Pdf => 2,
            FileKind::Doc => 3,
            FileKind::Audio => 4,
            FileKind::Video => 5,
            FileKind::Code => 6,
        }
    }

    /// SQL Case for indexing file during initial passes.
    pub const INDEXING_ORDER_SQL: &'static str = "\
        CASE kind \
        WHEN 'screenshot' THEN 0 \
        WHEN 'image' THEN 1 \
        WHEN 'pdf' THEN 2 \
        WHEN 'doc' THEN 3 \
        WHEN 'audio' THEN 4 \
        WHEN 'video' THEN 5 \
        ELSE 6 END";

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Doc => "doc",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Code => "code",
        }
    }
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FileKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "screenshot" => Ok(Self::Screenshot),
            "image" => Ok(Self::Image),
            "pdf" => Ok(Self::Pdf),
            "doc" => Ok(Self::Doc),
            "audio" => Ok(Self::Audio),
            "video" => Ok(Self::Video),
            "code" => Ok(Self::Code),
            _ => Err(format!("Unknown file kind: {}", s)),
        }
    }
}

/// Fundamental unit of search.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// File name and parent folder hierarchy (FTS5 keyword search only, no vector)
    Name,
    /// The image itself (photos, standalone images).
    Image,
    /// Text extracted from a screenshot via OCR.
    Ocr,
    /// Clean text extracted from a PDF page text layer.
    Page,
    /// A PDF page rendered as an image for visual embedding (scans, slides, complex scripts).
    #[serde(rename = "page_image")]
    PageImage,
    /// An overlapping window slice (~1,800 characters) of document text.
    Chunk,
    /// A 30-second window slice of audio (or a video soundtrack).
    Audio,
    /// An extracted video keyframe thumbnail.
    Frame,
    /// A syntactically aware stretch of whole code lines.
    Lines,
    /// A stretch of Jupyter notebook cells (markdown & code).
    Cells,
}

impl SegmentKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Image => "image",
            Self::Ocr => "ocr",
            Self::Page => "page",
            Self::PageImage => "page_image",
            Self::Chunk => "chunk",
            Self::Audio => "audio",
            Self::Frame => "frame",
            Self::Lines => "lines",
            Self::Cells => "cells",
        }
    }
}

impl fmt::Display for SegmentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl FromStr for SegmentKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "name" => Ok(Self::Name),
            "image" => Ok(Self::Image),
            "ocr" => Ok(Self::Ocr),
            "page" => Ok(Self::Page),
            "page_image" => Ok(Self::PageImage),
            "chunk" => Ok(Self::Chunk),
            "audio" => Ok(Self::Audio),
            "frame" => Ok(Self::Frame),
            "lines" => Ok(Self::Lines),
            "cells" => Ok(Self::Cells),
            _ => Err(format!("Unknown segment kind: {}", s)),
        }
    }
}

/// What produced a segments vector
///
/// Scores are normalized per modality because text vectors score higher
/// than image or audio vectors for the same semantic concept
/// (measured: 0.75-0.86 for text vs 0.67-0.78 for cross modal media).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    None,
    Text,
    Image,
    Audio,
}

impl Modality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }
}

impl fmt::Display for Modality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Modality {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "text" => Ok(Self::Text),
            "image" => Ok(Self::Image),
            "audio" => Ok(Self::Audio),
            _ => Err(format!("Unknown modality: {}", s)),
        }
    }
}

/// Canonical file metadata representation in SQLite table `files`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: i64,
    pub path: String,
    pub kind: FileKind,
    pub size: i64,
    pub mtime: f64,
    pub inode: u64,
    pub device: u64,
    pub state: String,
}

/// A piece of a file stored in SQLite table `segments`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentRecord {
    pub id: i64,
    pub file_id: i64,
    pub kind: SegmentKind,
    pub modality: Modality,
    pub text: Option<String>,
    pub loc: Option<i64>,
    pub loc_end: Option<i64>,
}

/// Search candidate produced by vector or FTS queries before final scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub file_id: i64,
    pub score: f32,
    pub segment_ids: Vec<i64>,
    pub modality: Modality,
}
