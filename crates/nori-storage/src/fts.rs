use crate::connection::{Database, StorageError};
use nori_text::SearchText;
use std::collections::HashSet;

/// Stopwords and modality filter keywords dropped from lexical queries.
pub static STOPWORDS: &[&str] = &[
    // English function words
    "a",
    "an",
    "the",
    "of",
    "in",
    "on",
    "at",
    "to",
    "for",
    "with",
    "and",
    "or",
    "my",
    "me",
    "that",
    "this",
    "these",
    "those",
    "from",
    "about",
    "where",
    "what",
    "which",
    "who",
    "when",
    "is",
    "was",
    "are",
    "were",
    "be",
    "it",
    "its",
    "some",
    "any",
    // Modality and intent words
    "find",
    "show",
    "file",
    "files",
    "photo",
    "photos",
    "picture",
    "image",
    "images",
    "screenshot",
    "screenshots",
    "video",
    "videos",
    "pdf",
    "document",
    "documents",
    "recording",
    // Bengali common words (normalized)
    "এই",
    "ওই",
    "কী",
    "কি",
    "কে",
    "কেন",
    "কার",
    "কোথায়",
    "কখন",
    "কিভাবে",
    "যে",
    "যা",
    "যিনি",
    "থেকে",
    "হতে",
    "দ্বারা",
    "দিয়ে",
    "জন্য",
    "মতো",
    "করে",
    "হয়ে",
    "ছিল",
    "আছে",
    // Arabic common particles (normalized)
    "في",
    "من",
    "على",
    "الى",
    "إلى",
    "عن",
    "مع",
    "هذا",
    "هذه",
    "ذلك",
    "تلك",
    "التي",
    "الذي",
    "كان",
    "كانت",
    "يكون",
    "ما",
    "ماذا",
    "لماذا",
    "كيف",
    "اين",
    "أين",
];

/// A lexical search hit returned from FTS5.
#[derive(Debug, Clone, PartialEq)]
pub struct KeywordHit {
    pub file_id: i64,
    pub segment_id: i64,
    pub snippet: Option<String>,
    pub bm25_score: f64,
}

pub struct Fts;

impl Fts {
    /// Extracts content terms from a query string:
    /// - Normalizes and lowercases text.
    /// - Splits on non-alphanumeric boundaries.
    /// - Drops stopwords and modality words.
    /// - Drops tokens with fewer than 2 characters.
    /// - Preserves first-seen order while deduplicating.
    pub fn terms(text: &str) -> Vec<String> {
        let normalized = SearchText::normalized(text).to_lowercase();
        let stopwords: HashSet<&str> = STOPWORDS.iter().copied().collect();

        let mut seen = HashSet::new();
        let mut result = Vec::new();

        for token in normalized.split(|c: char| !c.is_alphanumeric()) {
            let trimmed = token.trim();
            if trimmed.chars().count() >= 2
                && !stopwords.contains(trimmed)
                && seen.insert(trimmed.to_string())
            {
                result.push(trimmed.to_string());
            }
        }

        result
    }

    /// Formats terms into FTS5-safe quoted patterns.
    /// - Appends '*' to terms that inflect (Bengali, Arabic).
    /// - Appends '*' to the final term if `prefix_last == true` (typing as you search).
    pub fn patterns(terms: &[String], prefix_last: bool) -> Vec<String> {
        let count = terms.len();
        terms
            .iter()
            .enumerate()
            .map(|(i, term)| {
                let is_last = i == count - 1;
                let inflects = SearchText::inflects(term);
                let is_prefix = (prefix_last && is_last) || inflects;

                if is_prefix {
                    format!("\"{}\"*", term)
                } else {
                    format!("\"{}\"", term)
                }
            })
            .collect()
    }

    /// Generates an FTS5 MATCH query expression (OR-joined patterns).
    pub fn build_match_query(text: &str, prefix_last: bool) -> Option<String> {
        let terms = Self::terms(text);
        if terms.is_empty() {
            return None;
        }
        let patterns = Self::patterns(&terms, prefix_last);
        Some(patterns.join(" OR "))
    }

    /// Returns true if the query consists of codes, product IDs ("TRX50", "K7Q2LM"),
    /// or pure numbers of 4+ digits ("4815162342").
    ///
    /// Such queries carry zero semantic meaning for neural embeddings, signaling
    /// that the search engine should double keyword weights and halve vector weights.
    pub fn is_lookup(terms: &[String]) -> bool {
        if terms.is_empty() {
            return false;
        }

        terms.iter().all(|term| {
            let has_digit = term.chars().any(|c| c.is_ascii_digit());
            let has_letter = term.chars().any(|c| c.is_alphabetic());
            let is_long_num = term.chars().count() >= 4 && term.chars().all(|c| c.is_ascii_digit());

            has_digit && (has_letter || is_long_num)
        })
    }

    /// Executes an FTS5 lexical query with BM25 asymmetric weighting (name: 3.0, body: 1.0).
    pub fn search(
        db: &Database,
        match_expr: &str,
        limit: usize,
    ) -> Result<Vec<KeywordHit>, StorageError> {
        // Pre-compute query terms and convert to &[&str] once outside the loop
        let query_terms = Self::terms(match_expr);
        let term_refs: Vec<&str> = query_terms.iter().map(|s| s.as_str()).collect();

        let mut stmt = db.raw().prepare_cached(
            "SELECT s.file, s.id, snippet(fts, 1, '«', '»', '…', 12), fts.original, bm25(fts, 3.0, 1.0) AS rank
             FROM fts
             JOIN segments s ON s.id = fts.rowid
             WHERE fts MATCH ?
             ORDER BY rank ASC
             LIMIT ?;",
        )?;

        let mut rows = stmt.query(rusqlite::params![match_expr, limit as i64])?;
        let mut hits = Vec::new();

        while let Some(row) = rows.next()? {
            let file_id: i64 = row.get(0)?;
            let segment_id: i64 = row.get(1)?;
            let snippet_str: Option<String> = row.get(2)?;
            let original_text: Option<String> = row.get(3)?;
            let bm25_score: f64 = row.get(4)?;

            // Prefer original unindexed text if available for snippet reconstruction
            let snippet = if let Some(orig) = original_text {
                SearchText::snippet(&orig, &term_refs, 12).or(snippet_str)
            } else {
                snippet_str
            };

            hits.push(KeywordHit {
                file_id,
                segment_id,
                snippet,
                bm25_score,
            });
        }

        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Schema;

    #[test]
    fn test_terms_and_stopword_removal() {
        let query = "find a screenshot of my tax invoice for 2024";
        let terms = Fts::terms(query);
        // "find", "a", "screenshot", "of", "my", "for" are dropped
        assert_eq!(terms, vec!["tax", "invoice", "2024"]);
    }

    #[test]
    fn test_pattern_prefix_and_inflection() {
        let terms = vec!["report".to_string(), "বাংলা".to_string(), "invoi".to_string()];
        let patterns = Fts::patterns(&terms, true);

        assert_eq!(patterns[0], "\"report\"");
        // "বাংলা" inflects -> gets wildcard
        assert_eq!(patterns[1], "\"বাংলা\"*");
        // "invoi" is last and prefix_last is true -> gets wildcard
        assert_eq!(patterns[2], "\"invoi\"*");
    }

    #[test]
    fn test_is_lookup() {
        // Alphanumeric product ID
        assert!(Fts::is_lookup(&["TRX50".to_string()]));
        // 5-digit order number
        assert!(Fts::is_lookup(&["48151".to_string()]));
        // Plain words do not trigger lookup
        assert!(!Fts::is_lookup(&[
            "quantum".to_string(),
            "computing".to_string()
        ]));
    }

    #[test]
    fn test_bm25_search_name_outranks_body() {
        let mut db = Database::open_in_memory().unwrap();
        Schema::migrate(&mut db).unwrap();

        // File 1: word "quarterly" in body only
        db.raw().execute("INSERT INTO files(path, kind, size, mtime, inode, device) VALUES('/docs/notes.txt', 'doc', 10, 1.0, 1, 1);", []).unwrap();
        db.raw()
            .execute(
                "INSERT INTO segments(file, kind, modality) VALUES(1, 'chunk', 'none');",
                [],
            )
            .unwrap();
        db.raw().execute("INSERT INTO fts(rowid, name, body) VALUES(1, 'notes.txt', 'contains quarterly financial review');", []).unwrap();

        // File 2: word "quarterly" in file name (3.0 weight)
        db.raw().execute("INSERT INTO files(path, kind, size, mtime, inode, device) VALUES('/docs/quarterly_report.pdf', 'pdf', 10, 1.0, 2, 1);", []).unwrap();
        db.raw()
            .execute(
                "INSERT INTO segments(file, kind, modality) VALUES(2, 'name', 'none');",
                [],
            )
            .unwrap();
        db.raw()
            .execute(
                "INSERT INTO fts(rowid, name, body) VALUES(2, 'quarterly report pdf', 'docs');",
                [],
            )
            .unwrap();

        let query = Fts::build_match_query("quarterly", false).unwrap();
        let hits = Fts::search(&db, &query, 10).unwrap();

        assert_eq!(hits.len(), 2);
        // In SQLite FTS5 BM25, lower/more negative score is ranked better
        // File 2 (name match, weight 3.0) must rank first!
        assert_eq!(hits[0].file_id, 2);
        assert_eq!(hits[1].file_id, 1);
        assert!(hits[0].bm25_score < hits[1].bm25_score);
    }
}
