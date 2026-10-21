use crate::hit::SearchHit;
use crate::ranking::{self, is_lookup, kind_hints, patterns, terms};
use nori_core::options::IndexOptions;
use nori_core::types::{FileKind, Modality, SegmentKind};
use nori_core::vectors::{VectorStorage, Vectors};
use nori_extract::code::CodeLookup;
use nori_extract::doc::excerpt;
use nori_storage::store::IndexStore;
use nori_text::normalize::SearchText;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("storage error: {0}")]
    Storage(#[from] nori_storage::connection::StorageError),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("search error: {0}")]
    Message(String),
}

/// Hybrid search engine combining EmbeddingGemma dense vectors and SQLite FTS5 BM25.
pub struct Searcher {
    pub store: IndexStore,
    matrix: Option<VectorStorage>,
    row_file: Vec<i64>,
    row_segment: Vec<i64>,
    row_modality: Vec<Modality>,
    row_sets_scale: Vec<bool>,
    file_kind: HashMap<i64, FileKind>,
    pub vector_source: Option<String>,
    pub vectors_loaded: bool,
    pub is_code: bool,
}

impl Searcher {
    pub const WORD_WEIGHT: f64 = 2.0;
    pub const CODE_WORD_WEIGHT: f64 = 1.0;

    /// Initializes a new Searcher wrapping an IndexStore.
    pub fn new(store: IndexStore, load_vectors: bool, is_code: bool) -> Result<Self, SearchError> {
        let mut s = Self {
            store,
            matrix: None,
            row_file: Vec::new(),
            row_segment: Vec::new(),
            row_modality: Vec::new(),
            row_sets_scale: Vec::new(),
            file_kind: HashMap::new(),
            vector_source: None,
            vectors_loaded: false,
            is_code,
        };

        if load_vectors {
            s.reload()?;
        } else {
            s.reload_files()?;
        }

        Ok(s)
    }

    /// Reloads all vector embeddings and file mappings from SQLite into contiguous memory.
    pub fn reload(&mut self) -> Result<(), SearchError> {
        self.release_vectors();
        self.reload_files()?;

        let db = self.store.db();
        let raw = db.raw();

        let rows: i64 = raw.query_row(
            "SELECT COUNT(*) FROM segments WHERE vec IS NOT NULL;",
            [],
            |r| r.get(0),
        )?;

        if rows == 0 {
            self.vectors_loaded = true;
            return Ok(());
        }

        let total_rows = rows as usize;
        let mut matrix = match VectorStorage::new(total_rows) {
            Ok(m) => m,
            Err(_) => {
                self.vectors_loaded = true;
                return Ok(());
            }
        };

        let row_bytes = Vectors::DIMENSION * std::mem::size_of::<half::f16>();
        let steps = IndexOptions::default();

        let mut stmt = raw.prepare_cached(
            "SELECT s.id, s.file, s.modality, s.vec, s.kind, s.loc, f.kind,
                    CAST(json_extract(f.info, '$.pages') AS INTEGER)
             FROM segments s JOIN files f ON f.id = s.file
             WHERE s.vec IS NOT NULL ORDER BY s.id;",
        )?;

        let mut query_rows = stmt.query([])?;

        while let Some(row) = query_rows.next()? {
            let blob: Vec<u8> = match row.get(3) {
                Ok(b) => b,
                Err(_) => continue,
            };

            if blob.len() != row_bytes || self.row_file.len() >= total_rows {
                continue;
            }

            let seg_id: i64 = row.get(0)?;
            let file_id: i64 = row.get(1)?;
            let mod_str: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let modality: Modality = mod_str.parse().unwrap_or(Modality::None);

            let s_kind_str: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
            let s_loc: Option<f64> = row.get(5)?;
            let f_kind_str: String = row.get::<_, Option<String>>(6)?.unwrap_or_default();
            let pages: Option<i64> = row.get(7)?;

            let row_idx = self.row_file.len();
            matrix.copy_row_bytes(row_idx, &blob);

            self.row_segment.push(seg_id);
            self.row_file.push(file_id);
            self.row_modality.push(modality);

            let place = s_loc.unwrap_or(0.0) as usize;
            let later = if f_kind_str == "pdf" {
                s_kind_str != SegmentKind::Chunk.as_str()
                    && pages.unwrap_or(0) as usize > steps.big_pdf_pages
                    && place > steps.pdf_step
            } else {
                s_kind_str == SegmentKind::Chunk.as_str() && place > steps.doc_step
            };

            self.row_sets_scale.push(!later);
        }

        self.matrix = Some(matrix);
        self.vectors_loaded = true;
        Ok(())
    }

    /// Releases vector memory while keeping keyword FTS search operational.
    pub fn release_vectors(&mut self) {
        self.matrix = None;
        self.row_file.clear();
        self.row_segment.clear();
        self.row_modality.clear();
        self.row_sets_scale.clear();
        self.vectors_loaded = false;
    }

    /// Reloads file kinds and vector source metadata from the database.
    pub fn reload_files(&mut self) -> Result<(), SearchError> {
        self.file_kind.clear();
        let db = self.store.db();
        let raw = db.raw();

        let mut stmt = raw.prepare_cached("SELECT id, kind FROM files;")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let kind_str: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let kind: FileKind = kind_str.parse().unwrap_or(FileKind::Doc);
            self.file_kind.insert(id, kind);
        }

        self.vector_source = self.store.get_meta("vector_source")?;
        Ok(())
    }

    pub fn vector_count(&self) -> usize {
        self.row_file.len()
    }

    /// Scores all resident vector halves against a 768-dim L2-normalized float query vector using SIMD dot product.
    pub fn score(&mut self, query: &[f32]) -> Vec<f32> {
        let rows = self.row_file.len();
        let mut scores = vec![0.0f32; rows];
        if query.len() != Vectors::DIMENSION || rows == 0 {
            return scores;
        }

        if let Some(ref mut matrix) = self.matrix {
            let mut row = 0;
            while row < rows {
                let count = std::cmp::min(VectorStorage::BLOCK_ROWS, rows - row);
                matrix.score_batch(row, count, query, &mut scores[row..row + count]);
                row += count;
            }
        }

        scores
    }

    /// Performs multimodal hybrid search combining vector meaning and BM25 text coverage.
    pub fn search(
        &mut self,
        text: &str,
        query_vector: Option<&[f32]>,
        kinds: Option<&HashSet<FileKind>>,
        limit: usize,
        z_gate: f64,
        word_weight: Option<f64>,
        prefix_last: bool,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let weight = word_weight.unwrap_or(if self.is_code {
            Self::CODE_WORD_WEIGHT
        } else {
            Self::WORD_WEIGHT
        });

        if query_vector.is_some() && !self.vectors_loaded {
            self.reload()?;
        }

        // 1. Meaning scoring
        let maybe_scores = if let Some(query) = query_vector {
            if query.len() == Vectors::DIMENSION && !self.row_file.is_empty() {
                Some(self.score(query))
            } else {
                None
            }
        } else {
            None
        };

        let file_kinds = &self.file_kind;
        let allowed = |file_id: i64| -> bool {
            match kinds {
                Some(k) => file_kinds.get(&file_id).map(|f| k.contains(f)).unwrap_or(false),
                None => true,
            }
        };

        let mut best: HashMap<i64, (f64, f64, usize)> = HashMap::new(); // file -> (z, cosine, row)
        if let Some(ref scores) = maybe_scores {
            let meaning_z = ranking::meaning(scores, &self.row_modality, Some(&self.row_sets_scale));

            for (row, (&score, &z)) in scores.iter().zip(meaning_z.iter()).enumerate() {
                let file_id = self.row_file[row];
                if allowed(file_id) {
                    let prev_z = best.get(&file_id).map(|b| b.0).unwrap_or(f64::NEG_INFINITY);
                    if z > prev_z {
                        best.insert(file_id, (z, score as f64, row));
                    }
                }
            }
        }

        // 2. Keyword scoring (FTS5)
        let query_terms = terms(text);
        let identifier = if self.is_code {
            CodeLookup::identifier(text)
        } else {
            None
        };

        let mut words: Vec<(i64, i64, Option<String>)> = Vec::new(); // (file, segment, snippet)
        let mut coverage: HashMap<i64, f64> = HashMap::new();

        let mut patterns_list: Vec<String> = if let Some(ref id_tokens) = identifier {
            vec![CodeLookup::phrase(id_tokens)]
        } else {
            patterns(&query_terms, prefix_last)
        };

        if identifier.is_none() {
            for (index, term) in query_terms.iter().enumerate() {
                if term.chars().count() >= 3 && !patterns_list[index].ends_with('*') {
                    if !self.is_indexed(&patterns_list[index])? {
                        patterns_list[index].push('*');
                    }
                }
            }
        }

        if !patterns_list.is_empty() {
            let mut seen_files = HashSet::new();
            let or_query = patterns_list.join(" OR ");

            let db = self.store.db();
            let raw = db.raw();

            let mut fts_stmt = raw.prepare_cached(
                "SELECT s.file, s.id, snippet(fts, 1, '«', '»', '…', 12), fts.original
                 FROM fts JOIN segments s ON s.id = fts.rowid
                 WHERE fts MATCH ? ORDER BY bm25(fts, 3.0, 1.0) LIMIT 1000;",
            )?;

            let term_refs: Vec<&str> = query_terms.iter().map(|s| s.as_str()).collect();
            let mut rows = fts_stmt.query([&or_query])?;

            while let Some(row) = rows.next()? {
                let file_id: i64 = row.get(0)?;
                let seg_id: i64 = row.get(1)?;
                let snippet_col: Option<String> = row.get(2)?;
                let original_col: Option<String> = row.get(3)?;

                if words.len() >= 100 || !allowed(file_id) || !seen_files.insert(file_id) {
                    continue;
                }

                let snippet = original_col
                    .as_deref()
                    .and_then(|orig| SearchText::snippet(orig, &term_refs, 12))
                    .or(snippet_col);

                words.push((file_id, seg_id, snippet));
            }

            if !words.is_empty() {
                let file_id_strs: Vec<String> = words.iter().map(|w| w.0.to_string()).collect();
                let files_in_clause = file_id_strs.join(",");

                for pattern in &patterns_list {
                    let cov_sql = format!(
                        "SELECT DISTINCT s.file FROM fts JOIN segments s ON s.id = fts.rowid
                         WHERE fts MATCH ? AND s.file IN ({files_in_clause});"
                    );
                    let mut cov_stmt = raw.prepare(&cov_sql)?;
                    let mut cov_rows = cov_stmt.query([pattern])?;
                    let inc = 1.0 / (patterns_list.len() as f64);
                    while let Some(r) = cov_rows.next()? {
                        let fid: i64 = r.get(0)?;
                        *coverage.entry(fid).or_insert(0.0) += inc;
                    }
                }
            }
        }

        // 3. Candidate Qualification & Hybrid Score
        let lookup = is_lookup(&query_terms) || identifier.is_some();
        let (meaning_weight, words_weight) = if lookup {
            (0.5, weight * 2.0)
        } else {
            (1.0, weight)
        };

        let hinted = kind_hints(text);
        let mut candidates: HashSet<i64> = best
            .iter()
            .filter(|(_, v)| v.0 >= z_gate)
            .map(|(&k, _)| k)
            .collect();

        for (&file, &share) in &coverage {
            if share >= 0.5 {
                candidates.insert(file);
            }
        }

        let mut scored: Vec<(i64, f64)> = Vec::new();
        for file in candidates {
            let best_z = best.get(&file).map(|b| b.0).unwrap_or(0.0).max(0.0);
            let cov = coverage.get(&file).copied().unwrap_or(0.0);
            let mut score = meaning_weight * best_z + words_weight * cov.powf(1.5);

            if let Some(kind) = self.file_kind.get(&file) {
                if hinted.contains(kind) {
                    score += 1.0;
                }
            }
            scored.push((file, score));
        }

        // Sort by score descending, tie break by file id ascending
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut word_rank_map: HashMap<i64, (usize, &(i64, i64, Option<String>))> = HashMap::new();
        for (idx, item) in words.iter().enumerate() {
            word_rank_map.entry(item.0).or_insert((idx + 1, item));
        }

        let mut hits = Vec::new();
        let raw = self.store.db().raw();

        for (file_id, score) in scored.into_iter().take(limit) {
            let rank = hits.len();
            let semantic = best.get(&file_id).copied();
            let keyword = word_rank_map.get(&file_id).copied();

            let use_meaning = semantic
                .map(|s| s.0 >= z_gate || keyword.is_none())
                .unwrap_or(false);

            let segment_id = if use_meaning {
                semantic.map(|s| self.row_segment[s.2])
            } else {
                keyword.map(|k| k.1 .1)
            };

            let Some(mut seg_id) = segment_id else {
                continue;
            };

            let mut focus: Option<f64> = None;
            if self.is_code {
                let id_for_place = if rank < 10 { identifier.as_deref() } else { None };
                let words_here = keyword.map(|k| k.1 .1 == seg_id).unwrap_or(false) || identifier.is_some();
                let (resolved_seg, resolved_focus) = self.code_place(
                    file_id,
                    seg_id,
                    id_for_place,
                    text,
                    &query_terms,
                    words_here,
                )?;
                seg_id = resolved_seg;
                focus = resolved_focus;
            }

            // Retrieve file record info
            let mut path = String::new();
            let mut kind = FileKind::Doc;
            let mut modified = 0.0;
            let mut size: i64 = 0;
            let mut info: HashMap<String, String> = HashMap::new();

            if let Ok(mut stmt) = raw.prepare_cached("SELECT path, kind, mtime, size, info FROM files WHERE id = ?;") {
                if let Ok(mut rows) = stmt.query([file_id]) {
                    if let Ok(Some(row)) = rows.next() {
                        path = row.get(0).unwrap_or_default();
                        let k_str: String = row.get::<_, Option<String>>(1).unwrap_or_default().unwrap_or_default();
                        kind = k_str.parse().unwrap_or(FileKind::Doc);
                        modified = row.get(2).unwrap_or(0.0);
                        size = row.get(3).unwrap_or(0);
                        if let Ok(Some(json_str)) = row.get::<_, Option<String>>(4) {
                            if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&json_str) {
                                info = parsed;
                            }
                        }
                    }
                }
            }

            // Retrieve segment details
            let mut segment_kind = SegmentKind::Name;
            let mut loc: Option<f64> = None;
            let mut loc_end: Option<f64> = None;
            let mut stored_excerpt: Option<String> = None;

            if let Ok(mut stmt) = raw.prepare_cached("SELECT kind, loc, loc_end, excerpt FROM segments WHERE id = ?;") {
                if let Ok(mut rows) = stmt.query([seg_id]) {
                    if let Ok(Some(row)) = rows.next() {
                        let seg_str: String = row.get::<_, Option<String>>(0).unwrap_or_default().unwrap_or_default();
                        segment_kind = seg_str.parse().unwrap_or(SegmentKind::Name);
                        loc = row.get(1).ok();
                        loc_end = row.get(2).ok();
                        stored_excerpt = row.get(3).ok();
                    }
                }
            }

            let mut snippet = keyword
                .filter(|k| k.1 .1 == seg_id)
                .and_then(|k| k.1 .2.clone())
                .and_then(|s| if s.contains('«') { Some(excerpt(&s, 400)) } else { None });

            if snippet.is_none() && stored_excerpt.is_none() && loc.is_none() {
                if let Some(match_item) = keyword {
                    if let Some(ref text) = match_item.1 .2 {
                        if text.contains('«') {
                            let mut matched_kind: Option<String> = None;
                            if let Ok(mut stmt) = raw.prepare_cached("SELECT kind FROM segments WHERE id = ?;") {
                                if let Ok(mut rows) = stmt.query([match_item.1 .1]) {
                                    if let Ok(Some(row)) = rows.next() {
                                        matched_kind = row.get(0).ok();
                                    }
                                }
                            }
                            if matched_kind.as_deref() != Some(SegmentKind::Name.as_str()) {
                                snippet = Some(excerpt(text, 400));
                            }
                        }
                    }
                }
            }

            hits.push(SearchHit {
                path,
                kind,
                score,
                z: semantic.map(|s| s.0),
                cosine: semantic.map(|s| s.1),
                keyword_rank: keyword.map(|k| k.0),
                segment: segment_kind,
                segment_id: seg_id,
                loc,
                loc_end,
                excerpt: snippet.or(stored_excerpt),
                modified,
                size,
                info,
                focus,
            });
        }

        Ok(hits)
    }

    /// Checks if a keyword pattern exists in the FTS5 index.
    fn is_indexed(&self, pattern: &str) -> Result<bool, SearchError> {
        let db = self.store.db();
        let raw = db.raw();
        let mut stmt = raw.prepare_cached("SELECT 1 FROM fts WHERE fts MATCH ? LIMIT 1;")?;
        let mut rows = stmt.query([pattern])?;
        Ok(rows.next()?.is_some())
    }

    /// Fetches the text of a segment as it was written (original or body).
    pub fn text_of_segment(&self, id: i64) -> Result<Option<String>, SearchError> {
        let db = self.store.db();
        let raw = db.raw();
        let mut stmt = raw.prepare_cached("SELECT coalesce(original, body) FROM fts WHERE rowid = ?;")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }

    /// Resolves code definition location for an identifier or falls back to first line matching query terms.
    fn code_place(
        &self,
        file: i64,
        segment: i64,
        identifier: Option<&[String]>,
        query: &str,
        terms: &[String],
        words_here: bool,
    ) -> Result<(i64, Option<f64>), SearchError> {
        let mut name = query.trim();
        if let Some(stripped) = name.strip_suffix("()") {
            name = stripped;
        }

        let raw = self.store.db().raw();

        if let Some(tokens) = identifier {
            let phrase = CodeLookup::phrase(tokens);
            let mut stmt = raw.prepare_cached(
                "SELECT s.id, s.loc, coalesce(fts.original, fts.body)
                 FROM fts JOIN segments s ON s.id = fts.rowid
                 WHERE fts MATCH ? AND s.file = ? ORDER BY s.loc LIMIT 300;",
            )?;

            let mut rows = stmt.query(params![phrase, file])?;
            let mut best_def: Option<(i64, usize, usize)> = None; // (segment_id, line, strength)

            while let Some(row) = rows.next()? {
                if best_def.map(|b| b.2).unwrap_or(0) >= 2 {
                    break;
                }
                let sid: i64 = row.get(0)?;
                let loc: Option<f64> = row.get(1)?;
                let text: Option<String> = row.get(2)?;

                if let Some(t) = text {
                    let first_line = loc.unwrap_or(1.0) as usize;
                    if let Some((line, strength)) = CodeLookup::definition(name, &t, first_line) {
                        if strength > best_def.map(|b| b.2).unwrap_or(0) {
                            best_def = Some((sid, line, strength));
                        }
                    }
                }
            }

            if let Some((best_sid, best_line, _)) = best_def {
                return Ok((best_sid, Some(best_line as f64)));
            }
        }

        if !words_here && identifier.is_none() {
            return Ok((segment, None));
        }

        if let Some(seg_text) = self.text_of_segment(segment)? {
            let mut first = 1;
            let mut stmt = raw.prepare_cached("SELECT loc FROM segments WHERE id = ?;")?;
            let mut rows = stmt.query([segment])?;
            if let Some(row) = rows.next()? {
                if let Ok(Some(l)) = row.get::<_, Option<f64>>(0) {
                    first = l as usize;
                }
            }

            let words = if identifier.is_none() {
                terms.to_vec()
            } else {
                vec![name.to_lowercase()]
            };

            let line = CodeLookup::first_line(&words, &seg_text, first);
            return Ok((segment, line.map(|l| l as f64)));
        }

        Ok((segment, None))
    }
}
