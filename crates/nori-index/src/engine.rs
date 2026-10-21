use crate::events::{IndexEvent, StepEvent, SyncSummary};
use nori_core::options::IndexOptions;
use nori_core::prompts::Prompts;
use nori_core::types::{FileKind, Modality, SegmentKind};
use nori_crawl::crawler::Crawler;
use nori_extract::audio::AudioExtractor;
use nori_extract::code::CodeExtractor;
use nori_extract::doc::{excerpt, DocExtractor};
use nori_extract::image::ImageExtractor;
use nori_extract::pdf::PDFExtractor;
use nori_infer::traits::Embedder;
use nori_storage::store::{FileRecord, IndexStore};
use nori_text::normalize::SearchText;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("storage error: {0}")]
    Storage(#[from] nori_storage::connection::StorageError),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("inference error: {0}")]
    Inference(String),
    #[error("fingerprint mismatch: index={index}, current={current}")]
    FingerprintMismatch { index: String, current: String },
}

struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Indexer {
    pub store: IndexStore,
    pub options: IndexOptions,
    pub embedder: Box<dyn Embedder>,
    _scratch: ScratchDir,
    pub scratch_path: PathBuf,
}

impl Indexer {
    pub fn new(
        store: IndexStore,
        options: IndexOptions,
        embedder: Box<dyn Embedder>,
    ) -> Result<Self, IndexError> {
        let pid = std::process::id();
        let unique = uuid::Uuid::new_v4();
        let scratch = std::env::temp_dir()
            .join(format!("nori-{pid}"))
            .join(unique.to_string());
        std::fs::create_dir_all(&scratch)?;

        let mut idx = Self {
            store,
            options,
            embedder,
            _scratch: ScratchDir(scratch.clone()),
            scratch_path: scratch,
        };

        idx.check_fingerprint()?;
        Ok(idx)
    }

    /// Consumes the indexer and returns ownership of `IndexStore`.
    pub fn finish(self) -> IndexStore {
        self.store
    }

    /// Verifies index fingerprint matching model and settings.
    pub fn check_fingerprint(&mut self) -> Result<(), IndexError> {
        let current_source = self.embedder.info().vector_source();
        let current_fingerprint = format!("{} | {}", current_source, self.options.fingerprint());

        if let Some(indexed_fp) = self.store.get_meta("fingerprint")? {
            if indexed_fp != current_fingerprint {
                return Err(IndexError::FingerprintMismatch {
                    index: indexed_fp,
                    current: current_fingerprint,
                });
            }
        } else {
            self.store
                .set_meta("fingerprint", Some(&current_fingerprint))?;
            self.store
                .set_meta("vector_source", Some(&current_source))?;
        }

        Ok(())
    }

    /// Reconciles files on disk with the SQLite catalog.
    pub fn sync(&mut self, roots: &[PathBuf]) -> Result<SyncSummary, IndexError> {
        let crawl = Crawler::crawl(roots, &self.options);
        let mut summary = SyncSummary {
            crawl: crawl.clone(),
            ..Default::default()
        };

        let root_strings: Vec<String> = roots
            .iter()
            .map(|r| r.to_string_lossy().to_string())
            .collect();

        // Fetch existing files from database under these roots
        let mut existing = Vec::new();
        {
            let raw = self.store.db().raw();
            for root_prefix in &root_strings {
                let pattern = format!("{root_prefix}%");
                let mut stmt = raw.prepare_cached(
                    "SELECT id, path, kind, size, mtime, inode, device, state
                     FROM files WHERE path LIKE ?;",
                )?;
                let mut rows = stmt.query([&pattern])?;
                while let Some(row) = rows.next()? {
                    let kind_str: String = row.get(2)?;
                    existing.push(FileRecord {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        kind: kind_str.parse().unwrap_or(FileKind::Doc),
                        size: row.get(3)?,
                        mtime: row.get(4)?,
                        inode: row.get::<_, i64>(5)? as u64,
                        device: row.get::<_, i64>(6)? as u64,
                        state: row.get(7)?,
                    });
                }
            }
        }

        let mut by_path: HashMap<String, FileRecord> = HashMap::new();
        let mut by_inode: HashMap<(u64, u64), FileRecord> = HashMap::new();
        for file in existing {
            by_path.insert(file.path.clone(), file.clone());
            by_inode.insert((file.device, file.inode), file);
        }

        let seen: HashSet<String> = crawl.candidates.iter().map(|c| c.path.clone()).collect();
        let mut moved_from: HashSet<String> = HashSet::new();

        for candidate in crawl.candidates {
            if let Some(row) = by_path.get(&candidate.path) {
                if row.size != candidate.size
                    || (row.mtime - candidate.mtime).abs() > 0.001
                    || row.kind != candidate.kind
                {
                    self.store.delete_file(row.id)?;
                    let new_id = self.store.insert_file(
                        &candidate.path,
                        candidate.kind,
                        candidate.size,
                        candidate.mtime,
                        candidate.inode,
                        candidate.device,
                    )?;
                    summary.changed += 1;
                    summary.queued.push(new_id);
                } else {
                    summary.unchanged += 1;
                }
            } else if let Some(row) = by_inode.get(&(candidate.device, candidate.inode)) {
                if !seen.contains(&row.path)
                    && !moved_from.contains(&row.path)
                    && row.size == candidate.size
                    && (row.mtime - candidate.mtime).abs() < 0.001
                {
                    self.store.move_file(row.id, &candidate.path)?;
                    moved_from.insert(row.path.clone());
                    summary.moved += 1;
                } else {
                    let id = self.store.insert_file(
                        &candidate.path,
                        candidate.kind,
                        candidate.size,
                        candidate.mtime,
                        candidate.inode,
                        candidate.device,
                    )?;
                    summary.added += 1;
                    summary.queued.push(id);
                }
            } else {
                let id = self.store.insert_file(
                    &candidate.path,
                    candidate.kind,
                    candidate.size,
                    candidate.mtime,
                    candidate.inode,
                    candidate.device,
                )?;
                summary.added += 1;
                summary.queued.push(id);
            }
        }

        // Delete files that were not seen during crawl and not moved
        for (path, row) in by_path {
            if !seen.contains(&path) && !moved_from.contains(&path) {
                let exists = roots
                    .iter()
                    .any(|r| path.starts_with(&r.to_string_lossy().to_string()));
                if exists && !Path::new(&path).exists() {
                    self.store.delete_file(row.id)?;
                    summary.removed += 1;
                }
            }
        }

        Ok(summary)
    }

    /// Indexes pending files in batches until done or should_stop signals termination.
    pub fn run<F, S>(
        &mut self,
        limit: Option<usize>,
        only: Option<&[i64]>,
        mut should_stop: S,
        mut on_event: F,
    ) -> Result<HashMap<String, usize>, IndexError>
    where
        F: FnMut(IndexEvent),
        S: FnMut() -> bool,
    {
        let pending = self.store.pending_files(limit, only)?;
        let total = pending.len();
        let mut states: HashMap<String, usize> = HashMap::new();
        let mut done = 0;

        let batches = Self::batches(&pending);

        for batch in batches {
            if should_stop() {
                break;
            }

            let start = Instant::now();
            let results = self.process_batch(&batch)?;
            let elapsed = start.elapsed().as_secs_f64();

            for (offset, file) in batch.iter().enumerate() {
                let (state, note) = &results[offset];
                *states.entry(state.clone()).or_insert(0) += 1;
                done += 1;

                on_event(IndexEvent {
                    done,
                    total,
                    path: file.path.clone(),
                    kind: file.kind,
                    state: state.clone(),
                    seconds: elapsed / (batch.len() as f64),
                    note: note.clone(),
                });
            }
        }

        Ok(states)
    }

    /// Processes long files (multi-page PDFs or multi-chunk docs) past their initial pass.
    pub fn finish_long_files<F, S>(
        &mut self,
        mut should_stop: S,
        mut on_step: F,
    ) -> Result<HashMap<String, usize>, IndexError>
    where
        F: FnMut(StepEvent),
        S: FnMut() -> bool,
    {
        let mut states: HashMap<String, usize> = HashMap::new();
        let raw = self.store.db().raw();

        let mut unfinished_files = Vec::new();
        {
            let mut stmt = raw.prepare_cached(
                "SELECT id, path, kind, size, mtime, inode, device, state
                 FROM files WHERE state = 'partial' ORDER BY id ASC;",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let kind_str: String = row.get(2)?;
                unfinished_files.push(FileRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    kind: kind_str.parse().unwrap_or(FileKind::Doc),
                    size: row.get(3)?,
                    mtime: row.get(4)?,
                    inode: row.get::<_, i64>(5)? as u64,
                    device: row.get::<_, i64>(6)? as u64,
                    state: row.get(7)?,
                });
            }
        }

        let total_files = unfinished_files.len();
        let mut step_count = 0;

        for (idx, file) in unfinished_files.into_iter().enumerate() {
            if should_stop() {
                break;
            }

            let start_time = Instant::now();
            let (state, note) = match file.kind {
                FileKind::Doc => self.index_doc_step(&file, true)?,
                FileKind::Pdf => self.index_pdf_step(&file, true)?,
                _ => ("done".to_string(), None),
            };
            let elapsed = start_time.elapsed().as_secs_f64();

            step_count += 1;
            *states.entry(state.clone()).or_insert(0) += 1;

            on_step(StepEvent {
                done: step_count,
                total: total_files,
                files: total_files - idx,
                path: file.path.clone(),
                kind: file.kind,
                state,
                seconds: elapsed,
                note,
            });
        }

        Ok(states)
    }

    /// Groups images/screenshots 8 at a time; others 1 by 1.
    fn batches(queue: &[FileRecord]) -> Vec<Vec<FileRecord>> {
        let mut batches = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            let mut batch = vec![queue[i].clone()];
            if queue[i].kind == FileKind::Image || queue[i].kind == FileKind::Screenshot {
                while i + batch.len() < queue.len()
                    && batch.len() < 8
                    && queue[i + batch.len()].kind == queue[i].kind
                {
                    batch.push(queue[i + batch.len()].clone());
                }
            }
            i += batch.len();
            batches.push(batch);
        }
        batches
    }

    fn process_batch(
        &mut self,
        batch: &[FileRecord],
    ) -> Result<Vec<(String, Option<String>)>, IndexError> {
        let first = &batch[0];
        match first.kind {
            FileKind::Image | FileKind::Screenshot => self.index_images(batch),
            FileKind::Doc => {
                let res = self.index_doc_step(first, false)?;
                Ok(vec![res])
            }
            FileKind::Pdf => {
                let res = self.index_pdf_step(first, false)?;
                Ok(vec![res])
            }
            FileKind::Code => {
                let res = self.index_code(first)?;
                Ok(vec![res])
            }
            FileKind::Audio => {
                let res = self.index_audio(first)?;
                Ok(vec![res])
            }
            FileKind::Video => {
                let res = self.index_video(first)?;
                Ok(vec![res])
            }
        }
    }

    fn index_images(
        &mut self,
        batch: &[FileRecord],
    ) -> Result<Vec<(String, Option<String>)>, IndexError> {
        let mut results = Vec::with_capacity(batch.len());
        let is_screenshot = batch[0].kind == FileKind::Screenshot;
        let budget = if is_screenshot {
            self.options.screenshot_budget
        } else {
            self.options.photo_budget
        };

        let scratch = &self.scratch_path;
        let mut ready_files = Vec::new();
        let mut ready_indices = Vec::new();

        for (idx, file) in batch.iter().enumerate() {
            let path = Path::new(&file.path);
            if !path.exists() {
                self.store
                    .finish_file(file.id, "failed", Some("file not found"), None)?;
                results.push(("failed".to_string(), Some("file not found".to_string())));
                continue;
            }

            match ImageExtractor::prepare(
                path,
                if is_screenshot { 1536 } else { 1024 },
                self.options.min_image_side,
                &scratch,
                is_screenshot,
            ) {
                Ok(prep) => {
                    ready_files.push(prep);
                    ready_indices.push((idx, file.id));
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let state = if err_str.contains("too small") {
                        "skipped"
                    } else {
                        "failed"
                    };
                    self.store
                        .finish_file(file.id, state, Some(&err_str), None)?;
                    results.push((state.to_string(), Some(err_str)));
                }
            }
        }

        if !ready_files.is_empty() {
            let paths: Vec<String> = ready_files
                .iter()
                .map(|p| p.path.to_string_lossy().to_string())
                .collect();

            let embeddings = self
                .embedder
                .embed_images(&paths, budget)
                .map_err(|e| IndexError::Inference(e.to_string()))?;

            for (i, (&(_orig_idx, file_id), emb)) in
                ready_indices.iter().zip(embeddings.iter()).enumerate()
            {
                let prep = &ready_files[i];
                if let Some(ref vec) = emb.vector {
                    self.store.add_segment(
                        file_id,
                        SegmentKind::Image,
                        Modality::Image,
                        None,
                        None,
                        None,
                        None,
                        Some(vec.as_slice()),
                    )?;

                    let mut info = HashMap::new();
                    info.insert("width".to_string(), prep.width.to_string());
                    info.insert("height".to_string(), prep.height.to_string());
                    if let Some(ref t) = prep.taken {
                        info.insert("taken".to_string(), t.clone());
                    }

                    let info_json = serde_json::to_string(&info).unwrap_or_default();
                    self.store
                        .finish_file(file_id, "done", None, Some(&info_json))?;
                    results.push(("done".to_string(), None));
                } else {
                    let err = emb.error.as_deref().unwrap_or("embedding failed");
                    self.store
                        .finish_file(file_id, "failed", Some(err), None)?;
                    results.push(("failed".to_string(), Some(err.to_string())));
                }
            }
        }

        Ok(results)
    }

    fn index_doc_step(
        &mut self,
        file: &FileRecord,
        later: bool,
    ) -> Result<(String, Option<String>), IndexError> {
        let path = Path::new(&file.path);
        if !path.exists() {
            self.store
                .finish_file(file.id, "failed", Some("file not found"), None)?;
            return Ok(("failed".to_string(), Some("file not found".to_string())));
        }

        let doc_text = match DocExtractor::text(path) {
            Ok(t) => t,
            Err(e) => {
                let err_str = e.to_string();
                self.store
                    .finish_file(file.id, "failed", Some(&err_str), None)?;
                return Ok(("failed".to_string(), Some(err_str)));
            }
        };

        let chunks = DocExtractor::chunks(&doc_text, 1800, 200, self.options.max_chunks + 1);
        if chunks.is_empty() {
            self.store
                .finish_file(file.id, "skipped", Some("no text"), None)?;
            return Ok(("skipped".to_string(), Some("no text".to_string())));
        }

        let total = chunks.len();
        let end = total.min(self.options.max_chunks);

        let start = if later {
            self.store.last_place_of(file.id)?.unwrap_or(0)
        } else {
            0
        };

        let step_size = self.options.doc_step;
        let step_end = (start + step_size).min(end);
        let slice = &chunks[start..step_end];

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");

        let worded_indices: Vec<usize> = slice
            .iter()
            .enumerate()
            .filter(|(_, c)| SearchText::has_words(c))
            .map(|(i, _)| i)
            .collect();

        let prompts: Vec<String> = worded_indices
            .iter()
            .map(|&i| Prompts::document(Some(title), &slice[i]))
            .collect();

        let vectors = if !prompts.is_empty() {
            self.embedder
                .embed_texts(&prompts)
                .map_err(|e| IndexError::Inference(e.to_string()))?
        } else {
            Vec::new()
        };

        let mut vec_map = HashMap::new();
        for (&idx, v) in worded_indices.iter().zip(vectors.into_iter()) {
            vec_map.insert(idx, v);
        }

        for (offset, chunk) in slice.iter().enumerate() {
            let place = (start + offset + 1) as f64;
            let vec = vec_map.get(&offset);
            self.store.add_segment(
                file.id,
                SegmentKind::Chunk,
                Modality::Text,
                Some(place),
                None,
                Some(&excerpt(chunk, 400)),
                Some(chunk),
                vec.map(|v| v.as_slice()),
            )?;
        }

        let unread = step_end < end;
        let state = if unread { "partial" } else { "done" };

        let mut info = HashMap::new();
        info.insert("chunks".to_string(), end.to_string());
        info.insert("chars".to_string(), doc_text.chars().count().to_string());
        if unread {
            info.insert("read_to".to_string(), step_end.to_string());
            info.insert("read_end".to_string(), end.to_string());
        }

        let info_json = serde_json::to_string(&info).unwrap_or_default();
        self.store
            .finish_file(file.id, state, None, Some(&info_json))?;

        let note = format!("chunks {}–{} of {}", start + 1, step_end, end);
        Ok((state.to_string(), Some(note)))
    }

    fn index_pdf_step(
        &mut self,
        file: &FileRecord,
        later: bool,
    ) -> Result<(String, Option<String>), IndexError> {
        let path = Path::new(&file.path);
        if !path.exists() {
            self.store
                .finish_file(file.id, "failed", Some("file not found"), None)?;
            return Ok(("failed".to_string(), Some("file not found".to_string())));
        }

        let scratch = &self.scratch_path;
        let pdf_res = PDFExtractor::pages(
            path,
            |total| {
                let all = total.min(self.options.max_pages);
                if later {
                    let last = 0;
                    last..all.min(last + self.options.pdf_step)
                } else if total > self.options.big_pdf_pages {
                    0..self.options.pdf_step.min(all)
                } else {
                    0..all
                }
            },
            self.options.min_page_text,
            &scratch,
        );

        let read_pages = match pdf_res {
            Ok(p) => p,
            Err(e) => {
                let err_str = e.to_string();
                self.store
                    .finish_file(file.id, "failed", Some(&err_str), None)?;
                return Ok(("failed".to_string(), Some(err_str)));
            }
        };

        if read_pages.pages.is_empty() {
            self.store
                .finish_file(file.id, "skipped", Some("no pages"), None)?;
            return Ok(("skipped".to_string(), Some("no pages".to_string())));
        }

        let title = read_pages.title.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("document")
                .to_string()
        });

        for page in &read_pages.pages {
            let mut vec = None;
            if SearchText::has_words(&page.text) {
                let prompt = Prompts::document(Some(&title), &page.text);
                if let Ok(mut vs) = self.embedder.embed_texts(&[prompt]) {
                    if let Some(v) = vs.pop() {
                        vec = Some(v);
                    }
                }
            }

            self.store.add_segment(
                file.id,
                SegmentKind::Page,
                Modality::Text,
                Some(page.number as f64),
                None,
                Some(&excerpt(&page.text, 400)),
                Some(&page.text),
                vec.as_deref(),
            )?;
        }

        let mut info = HashMap::new();
        info.insert("pages".to_string(), read_pages.total.to_string());
        let info_json = serde_json::to_string(&info).unwrap_or_default();
        self.store
            .finish_file(file.id, "done", None, Some(&info_json))?;

        Ok((
            "done".to_string(),
            Some(format!("{} pages", read_pages.pages.len())),
        ))
    }

    fn index_code(&mut self, file: &FileRecord) -> Result<(String, Option<String>), IndexError> {
        let path = Path::new(&file.path);
        let code_res = match CodeExtractor::read(path, &self.options) {
            Ok(c) => c,
            Err(e) => {
                let err_str = e.to_string();
                self.store
                    .finish_file(file.id, "failed", Some(&err_str), None)?;
                return Ok(("failed".to_string(), Some(err_str)));
            }
        };

        if code_res.pieces.is_empty() {
            self.store
                .finish_file(file.id, "skipped", Some("no code"), None)?;
            return Ok(("skipped".to_string(), Some("no code".to_string())));
        }

        let worded_indices: Vec<usize> = code_res
            .pieces
            .iter()
            .enumerate()
            .filter(|(_, p)| SearchText::has_words(&p.text))
            .map(|(i, _)| i)
            .collect();

        let prompts: Vec<String> = worded_indices
            .iter()
            .map(|&i| Prompts::document(Some(&code_res.title), &code_res.pieces[i].text))
            .collect();

        let vectors = if !prompts.is_empty() {
            self.embedder
                .embed_texts(&prompts)
                .map_err(|e| IndexError::Inference(e.to_string()))?
        } else {
            Vec::new()
        };

        let mut vec_map = HashMap::new();
        for (&idx, v) in worded_indices.iter().zip(vectors.into_iter()) {
            vec_map.insert(idx, v);
        }

        for (offset, piece) in code_res.pieces.iter().enumerate() {
            let vec = vec_map.get(&offset);
            let summary = CodeExtractor::summary(&piece.text);
            self.store.add_segment(
                file.id,
                SegmentKind::Lines,
                Modality::Text,
                Some(piece.first as f64),
                Some(piece.last as f64),
                Some(&summary),
                Some(&piece.text),
                vec.map(|v| v.as_slice()),
            )?;
        }

        let mut info = HashMap::new();
        info.insert("lines".to_string(), code_res.lines.to_string());
        if let Some(r) = code_res.repo {
            info.insert("repo".to_string(), r);
        }

        let info_json = serde_json::to_string(&info).unwrap_or_default();
        self.store
            .finish_file(file.id, "done", None, Some(&info_json))?;
        let note = format!("{} stretches, {} lines", code_res.pieces.len(), code_res.lines);
        Ok(("done".to_string(), Some(note)))
    }

    fn index_audio(&mut self, file: &FileRecord) -> Result<(String, Option<String>), IndexError> {
        let path = Path::new(&file.path);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext != "wav" {
            self.store.finish_file(
                file.id,
                "skipped",
                Some("only WAV supported without ffmpeg"),
                None,
            )?;
            return Ok(("skipped".to_string(), Some("non-wav audio format".to_string())));
        }

        let scratch = &self.scratch_path;
        let (pieces, duration) = match AudioExtractor::windows_from_wav(path, &scratch) {
            Ok(res) => res,
            Err(e) => {
                let err_str = e.to_string();
                self.store
                    .finish_file(file.id, "failed", Some(&err_str), None)?;
                return Ok(("failed".to_string(), Some(err_str)));
            }
        };

        if pieces.is_empty() {
            self.store
                .finish_file(file.id, "skipped", Some("silent or too short"), None)?;
            return Ok(("skipped".to_string(), Some("silent or too short".to_string())));
        }

        let wav_paths: Vec<String> = pieces
            .iter()
            .map(|p| p.path.to_string_lossy().to_string())
            .collect();

        let embeddings = self
            .embedder
            .embed_audio(&wav_paths)
            .map_err(|e| IndexError::Inference(e.to_string()))?;

        for (piece, emb) in pieces.iter().zip(embeddings.into_iter()) {
            if let Some(ref v) = emb.vector {
                self.store.add_segment(
                    file.id,
                    SegmentKind::Audio,
                    Modality::Audio,
                    Some(piece.start),
                    Some(piece.end),
                    None,
                    None,
                    Some(v.as_slice()),
                )?;
            }
        }

        let mut info = HashMap::new();
        info.insert("duration".to_string(), format!("{duration:.1}"));
        let info_json = serde_json::to_string(&info).unwrap_or_default();
        self.store
            .finish_file(file.id, "done", None, Some(&info_json))?;

        Ok(("done".to_string(), Some(format!("{} windows", pieces.len()))))
    }

    fn index_video(&mut self, file: &FileRecord) -> Result<(String, Option<String>), IndexError> {
        self.store
            .finish_file(file.id, "skipped", Some("video requires external demuxer"), None)?;
        Ok(("skipped".to_string(), Some("video demuxer placeholder".to_string())))
    }
}
