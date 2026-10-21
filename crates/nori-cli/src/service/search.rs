//! SearchService: JSON-lines searcher background process.

use crate::protocol::{SearchHit, SearchRequest, SearchResponse, SearchTiming};
use nori_core::types::FileKind;
use nori_infer::{EmbeddingWorker, MockEmbedder, WorkerConfig};
use nori_search::Searcher;
use nori_storage::store::IndexStore;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct SearchService {
    db_path: PathBuf,
    searcher: Searcher,
    embedder: Box<dyn nori_infer::traits::Embedder>,
    vector_cache: HashMap<String, Vec<f32>>,
    quitting: bool,
    is_code: bool,
}

impl SearchService {
    pub fn new(db_path: &Path, is_code: bool) -> Result<Self, String> {
        let store = IndexStore::open(db_path).map_err(|e| format!("failed to open DB: {e}"))?;
        let searcher = Searcher::new(store, true, is_code)
            .map_err(|e| format!("failed to init searcher: {e}"))?;

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

        Ok(Self {
            db_path: db_path.to_path_buf(),
            searcher,
            embedder,
            vector_cache: HashMap::new(),
            quitting: false,
            is_code,
        })
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Process a single incoming JSON line and produce a single outgoing JSON response string.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let req: SearchRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = SearchResponse {
                    error: Some(format!("invalid request JSON: {e}")),
                    ..Default::default()
                };
                return serde_json::to_string(&err_resp).ok();
            }
        };

        let resp = self.handle_request(req);
        serde_json::to_string(&resp).ok()
    }

    fn handle_request(&mut self, req: SearchRequest) -> SearchResponse {
        match req.op.as_str() {
            "search" => {
                let query = req.q.unwrap_or_default();
                let limit = req.limit.unwrap_or(10);
                let meaning = req.meaning;

                let start_embed = Instant::now();
                let query_vec = if meaning {
                    if let Some(cached) = self.vector_cache.get(&query) {
                        Some(cached.clone())
                    } else {
                        match self.embedder.embed_texts(&[query.clone()]) {
                            Ok(mut vs) => vs.pop(),
                            Err(_) => None,
                        }
                    }
                } else {
                    None
                };
                let embed_ms = start_embed.elapsed().as_secs_f64() * 1000.0;

                let filter_kinds: Option<HashSet<FileKind>> = req.kinds.map(|ks| {
                    ks.iter()
                        .filter_map(|k| match k.to_lowercase().as_str() {
                            "screenshot" => Some(FileKind::Screenshot),
                            "image" => Some(FileKind::Image),
                            "pdf" => Some(FileKind::Pdf),
                            "doc" => Some(FileKind::Doc),
                            "audio" => Some(FileKind::Audio),
                            "video" => Some(FileKind::Video),
                            _ => None,
                        })
                        .collect()
                });

                let start_search = Instant::now();
                let search_res = self.searcher.search(
                    &query,
                    query_vec.as_deref(),
                    filter_kinds.as_ref(),
                    limit,
                    1.5,
                    None,
                    req.prefix_last,
                );
                let search_ms = start_search.elapsed().as_secs_f64() * 1000.0;

                match search_res {
                    Ok(hits) => {
                        let strong_count = hits.iter().filter(|h| h.score >= 0.5).count();
                        let search_hits = hits
                            .into_iter()
                            .map(|h| {
                                let title = Path::new(&h.path)
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| h.path.clone());
                                SearchHit {
                                    path: h.path.clone(),
                                    title,
                                    score: h.score as f32,
                                    kind: h.kind.as_str().to_string(),
                                    snippet: h.excerpt.clone(),
                                    excerpt: h.excerpt,
                                    segment: Some(h.segment.as_str().to_string()),
                                    segment_id: Some(h.segment_id),
                                    loc: h.loc,
                                    loc_end: h.loc_end,
                                    focus: h.focus,
                                    page_number: h.loc.map(|l| l as i32),
                                    file_size: Some(h.size as u64),
                                    size: Some(h.size as u64),
                                    modified: Some(h.modified),
                                    modified_ms: Some((h.modified * 1000.0) as i64),
                                    z: h.z,
                                    cosine: h.cosine,
                                    keyword_rank: h.keyword_rank,
                                    info: Some(h.info),
                                }
                            })
                            .collect();

                        SearchResponse {
                            id: req.id,
                            hits: Some(search_hits),
                            strong: Some(strong_count),
                            meaning: Some(meaning && query_vec.is_some()),
                            ms: Some(SearchTiming {
                                embed: Some(embed_ms),
                                search: Some(search_ms),
                            }),
                            ok: Some(true),
                            ..Default::default()
                        }
                    }
                    Err(e) => SearchResponse {
                        id: req.id,
                        error: Some(format!("search failed: {e}")),
                        ..Default::default()
                    },
                }
            }
            "embed_ahead" => {
                if let Some(query) = req.q {
                    if !self.vector_cache.contains_key(&query) {
                        if let Ok(mut vs) = self.embedder.embed_texts(&[query.clone()]) {
                            if let Some(v) = vs.pop() {
                                self.vector_cache.insert(query, v);
                            }
                        }
                    }
                }
                SearchResponse {
                    id: req.id,
                    ok: Some(true),
                    ..Default::default()
                }
            }
            "reload" => {
                self.vector_cache.clear();
                let _ = self.searcher.reload();
                SearchResponse {
                    id: req.id,
                    ok: Some(true),
                    ..Default::default()
                }
            }
            "status" => {
                let counts = self.searcher.store.counts().unwrap_or_default();
                let stats = serde_json::json!({
                    "code": self.is_code,
                    "files": counts.files,
                    "searchable": counts.searchable,
                    "pending": counts.pending,
                    "unfinished": counts.unfinished,
                    "cached_vectors": self.vector_cache.len(),
                    "vectors_loaded": self.searcher.vectors_loaded,
                    "healthy": true,
                });
                SearchResponse {
                    id: req.id,
                    files: Some(counts.files),
                    searchable: Some(counts.searchable),
                    pending: Some(counts.pending),
                    unfinished: Some(counts.unfinished),
                    status: Some(stats),
                    ok: Some(true),
                    ..Default::default()
                }
            }
            "render" => {
                let file_path_str = match req.path {
                    Some(ref p) if !p.is_empty() => p.clone(),
                    _ => {
                        return SearchResponse {
                            id: req.id,
                            error: Some("missing path for render".into()),
                            ..Default::default()
                        };
                    }
                };

                let file_path = Path::new(&file_path_str);
                if !file_path.exists() {
                    return SearchResponse {
                        id: req.id,
                        error: Some(format!("file not found: {file_path_str}")),
                        ..Default::default()
                    };
                }

                let previews_dir = self.db_path.parent().unwrap_or(Path::new(".")).join("previews");
                let _ = std::fs::create_dir_all(&previews_dir);

                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                file_path_str.hash(&mut hasher);
                let path_hash = hasher.finish();

                let loc_val = req.loc.unwrap_or(0.0);
                let px = req.pixels.unwrap_or(300).clamp(32, 2048);
                let out_name = format!("{:016x}_{}_{}.jpg", path_hash, (loc_val * 100.0) as i64, px);
                let out_path = previews_dir.join(&out_name);

                let needs_render = if out_path.exists() {
                    match (file_path.metadata(), out_path.metadata()) {
                        (Ok(sm), Ok(tm)) => {
                            let src_mtime = sm.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let thumb_mtime = tm.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            src_mtime > thumb_mtime
                        }
                        _ => true,
                    }
                } else {
                    true
                };

                if !needs_render {
                    return SearchResponse {
                        id: req.id,
                        image: Some(out_path.to_string_lossy().to_string()),
                        ok: Some(true),
                        ..Default::default()
                    };
                }

                if let Ok(img) = image::open(file_path) {
                    let thumb = img.thumbnail(px, px);
                    if thumb.save_with_format(&out_path, image::ImageFormat::Jpeg).is_ok() {
                        return SearchResponse {
                            id: req.id,
                            image: Some(out_path.to_string_lossy().to_string()),
                            ok: Some(true),
                            ..Default::default()
                        };
                    }
                }

                let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                if req.kind.as_deref() == Some("video") || ["mp4", "mkv", "avi", "mov", "webm", "wmv", "m4v"].contains(&ext.as_str()) {
                    let loc_str = format!("{:.2}", req.loc.unwrap_or(0.5));
                    #[cfg(windows)]
                    let mut cmd = std::process::Command::new("ffmpeg");
                    #[cfg(not(windows))]
                    let mut cmd = std::process::Command::new("ffmpeg");

                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                    }

                    let ffmpeg_res = cmd
                        .args([
                            "-y",
                            "-ss", &loc_str,
                            "-i", &file_path_str,
                            "-vframes", "1",
                            "-vf", &format!("scale={px}:{px}:force_original_aspect_ratio=decrease"),
                            out_path.to_str().unwrap_or("")
                        ])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    if let Ok(s) = ffmpeg_res {
                        if s.success() && out_path.exists() {
                            return SearchResponse {
                                id: req.id,
                                image: Some(out_path.to_string_lossy().to_string()),
                                ok: Some(true),
                                ..Default::default()
                            };
                        }
                    }
                }

                if req.kind.as_deref() == Some("pdf") || ext == "pdf" {
                    let page_num = (req.loc.unwrap_or(1.0) as i64).max(1);
                    let prefix = out_path.with_extension("");
                    #[cfg(windows)]
                    let mut cmd = std::process::Command::new("pdftoppm");
                    #[cfg(not(windows))]
                    let mut cmd = std::process::Command::new("pdftoppm");

                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                    }

                    let pdftoppm_res = cmd
                        .args([
                            "-jpeg",
                            "-f", &page_num.to_string(),
                            "-l", &page_num.to_string(),
                            "-scale-to", &px.to_string(),
                            &file_path_str,
                            prefix.to_str().unwrap_or("")
                        ])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    if let Ok(s) = pdftoppm_res {
                        if s.success() {
                            let generated_dash = PathBuf::from(format!("{}-{}.jpg", prefix.display(), page_num));
                            let generated_pad = PathBuf::from(format!("{}-{:02}.jpg", prefix.display(), page_num));
                            let generated_pad3 = PathBuf::from(format!("{}-{:03}.jpg", prefix.display(), page_num));
                            let actual = if generated_dash.exists() {
                                Some(generated_dash)
                            } else if generated_pad.exists() {
                                Some(generated_pad)
                            } else if generated_pad3.exists() {
                                Some(generated_pad3)
                            } else {
                                None
                            };

                            if let Some(src) = actual {
                                let _ = std::fs::rename(src, &out_path);
                                if out_path.exists() {
                                    return SearchResponse {
                                        id: req.id,
                                        image: Some(out_path.to_string_lossy().to_string()),
                                        ok: Some(true),
                                        ..Default::default()
                                    };
                                }
                            }
                        }
                    }
                }

                SearchResponse {
                    id: req.id,
                    error: Some("cannot render thumbnail".into()),
                    ok: Some(false),
                    ..Default::default()
                }
            }
            "quit" => {
                self.quitting = true;
                SearchResponse {
                    id: req.id,
                    ok: Some(true),
                    ..Default::default()
                }
            }
            unknown => SearchResponse {
                id: req.id,
                error: Some(format!("unknown operation '{unknown}'")),
                ..Default::default()
            },
        }
    }

    /// Run the interactive JSON-lines request loop.
    pub fn run_loop<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> std::io::Result<()> {
        let index_dir = self
            .db_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_string_lossy()
            .to_string();
        let ready_ev = serde_json::json!({
            "event": "ready",
            "index": index_dir,
        });
        writeln!(writer, "{}", ready_ev)?;
        writer.flush()?;

        for line in reader.lines() {
            let line = line?;
            if let Some(resp) = self.handle_line(&line) {
                writeln!(writer, "{}", resp)?;
                writer.flush()?;
            }
            if self.quitting {
                break;
            }
        }
        Ok(())
    }
}
