use crate::hit::SearchHit;
use nori_core::types::{FileKind, Modality};
use nori_text::normalize::SearchText;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Multilingual stopwords dropped from search terms (English, Bengali, Arabic).
pub static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "of", "in", "on", "at", "to", "for", "with", "and", "or", "my", "me",
        "that", "this", "these", "those", "from", "about", "where", "what", "which", "who", "when",
        "is", "was", "are", "were", "be", "it", "its", "some", "any", "find", "show", "file",
        "files", "photo", "photos", "picture", "image", "images", "screenshot", "screenshots",
        "video", "videos", "pdf", "document", "documents", "recording",
        // Bengali (normalized, as `terms` sees them)
        "এবং", "ও", "এই", "সেই", "একটি", "আমার", "থেকে", "জন্য", "সাথে", "মধ্যে", "কোথায়",
        "কোন", "কী", "কি", "যে", "ছবি", "স্ক্রিনশট", "ভিডিও", "ফাইল", "পিডিএফ",
        // Arabic
        "في", "من", "على", "الى", "عن", "مع", "هذا", "هذه", "ذلك", "التي", "الذي", "اين", "ما",
        "صورة", "صور", "لقطة", "شاشة", "فيديو", "ملف", "مستند",
    ]
    .into_iter()
    .collect()
});

/// Words indicating a user's modality intent (e.g. "screenshot of invoice", "sound of rain").
pub static KIND_WORDS: LazyLock<HashMap<&'static str, FileKind>> = LazyLock::new(|| {
    [
        ("screenshot", FileKind::Screenshot),
        ("screenshots", FileKind::Screenshot),
        ("photo", FileKind::Image),
        ("photos", FileKind::Image),
        ("picture", FileKind::Image),
        ("pictures", FileKind::Image),
        ("image", FileKind::Image),
        ("images", FileKind::Image),
        ("pdf", FileKind::Pdf),
        ("pdfs", FileKind::Pdf),
        ("paper", FileKind::Pdf),
        ("papers", FileKind::Pdf),
        ("document", FileKind::Doc),
        ("documents", FileKind::Doc),
        ("note", FileKind::Doc),
        ("notes", FileKind::Doc),
        ("video", FileKind::Video),
        ("videos", FileKind::Video),
        ("clip", FileKind::Video),
        ("movie", FileKind::Video),
        ("audio", FileKind::Audio),
        ("song", FileKind::Audio),
        ("podcast", FileKind::Audio),
        ("voice", FileKind::Audio),
        ("sound", FileKind::Audio),
        // Bengali
        ("স্ক্রিনশট", FileKind::Screenshot),
        ("ছবি", FileKind::Image),
        ("পিডিএফ", FileKind::Pdf),
        ("নোট", FileKind::Doc),
        ("ভিডিও", FileKind::Video),
        ("অডিও", FileKind::Audio),
        ("গান", FileKind::Audio),
        ("রেকর্ডিং", FileKind::Audio),
        // Arabic
        ("لقطة", FileKind::Screenshot),
        ("صورة", FileKind::Image),
        ("صور", FileKind::Image),
        ("مستند", FileKind::Doc),
        ("ملاحظات", FileKind::Doc),
        ("فيديو", FileKind::Video),
        ("مقطع", FileKind::Video),
        ("صوت", FileKind::Audio),
        ("تسجيل", FileKind::Audio),
        ("اغنية", FileKind::Audio),
    ]
    .into_iter()
    .collect()
});

/// Extracts content words of a query, normalized as keyword search keeps its text (`SearchText`).
/// Dropping stopwords and kind words; terms must have at least 2 unicode characters.
pub fn terms(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let norm = SearchText::normalized(text).to_lowercase();
    let mut result = Vec::new();

    for token in norm.split(|c: char| !c.is_alphanumeric()) {
        if token.chars().count() >= 2 && !STOPWORDS.contains(token) && seen.insert(token.to_string()) {
            result.push(token.to_string());
        }
    }
    result
}

/// FTS5 patterns for the query's words: quoted, and as prefixes for the last word while
/// it's being typed or for inflected languages (Bengali/Arabic).
pub fn patterns(terms: &[String], prefix_last: bool) -> Vec<String> {
    terms
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let is_prefix = (prefix_last && index == terms.len() - 1) || SearchText::inflects(term);
            if is_prefix {
                format!("\"{term}\"*")
            } else {
                format!("\"{term}\"")
            }
        })
        .collect()
}

/// Formats the query's words as an FTS5 OR-query.
pub fn fts_query(text: &str, prefix_last: bool) -> Option<String> {
    let t = terms(text);
    if t.is_empty() {
        None
    } else {
        Some(patterns(&t, prefix_last).join(" OR "))
    }
}

/// Detects file kinds requested in query text (e.g. "photo of dog", "ভিডিওতে গান").
pub fn kind_hints(text: &str) -> HashSet<FileKind> {
    let norm = SearchText::normalized(text).to_lowercase();
    let mut hints = HashSet::new();

    for word in norm.split(|c: char| !c.is_alphanumeric()) {
        if let Some(&kind) = KIND_WORDS.get(word) {
            hints.insert(kind);
        } else {
            // Bengali/Arabic case endings attach to the root kind word (e.g., "ভিডিওতে" starts with "ভিডিও")
            for (&key, &kind) in KIND_WORDS.iter() {
                if SearchText::inflects(key) && key.chars().count() >= 3 && word.starts_with(key) {
                    hints.insert(kind);
                }
            }
        }
    }
    hints
}

/// Detects if the query looks like a serial number, code ID, or exact alphanumeric token.
/// Queries with digits and letters (or 4+ digits) weigh words double and meaning half.
pub fn is_lookup(terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    terms.iter().all(|t| {
        let has_num = t.chars().any(|c| c.is_numeric());
        let has_letter = t.chars().any(|c| c.is_alphabetic());
        let count = t.chars().count();
        has_num && (has_letter || count >= 4)
    })
}

/// Calculates average cosine score for each modality. Small groups (< 20 vectors) take the global average.
pub fn modality_means(
    scores: &[f32],
    modality: &[Modality],
    sets_scale: Option<&[bool]>,
) -> HashMap<Modality, f64> {
    let mut sum: HashMap<Modality, f64> = HashMap::new();
    let mut count: HashMap<Modality, usize> = HashMap::new();

    for (row, (&score, &m)) in scores.iter().zip(modality.iter()).enumerate() {
        let include = sets_scale.and_then(|s| s.get(row).copied()).unwrap_or(true);
        if include {
            *sum.entry(m).or_insert(0.0) += score as f64;
            *count.entry(m).or_insert(0) += 1;
        }
    }

    let total_sum: f64 = sum.values().sum();
    let total_count: usize = count.values().sum();
    let all = total_sum / (total_count.max(1) as f64);

    let all_modalities = [
        Modality::Text,
        Modality::Image,
        Modality::Audio,
        Modality::None,
    ];

    let mut out = HashMap::new();
    for &m in &all_modalities {
        let n = count.get(&m).copied().unwrap_or(0);
        if n >= 20 {
            out.insert(m, sum.get(&m).copied().unwrap_or(0.0) / (n as f64));
        } else {
            out.insert(m, all);
        }
    }
    out
}

/// Converts raw cosine similarity scores into standardized z-scores centered by modality.
pub fn meaning(scores: &[f32], modality: &[Modality], sets_scale: Option<&[bool]>) -> Vec<f64> {
    if scores.is_empty() {
        return Vec::new();
    }

    let means = modality_means(scores, modality, sets_scale);
    let centered: Vec<f64> = scores
        .iter()
        .zip(modality.iter())
        .map(|(&s, &m)| (s as f64) - means.get(&m).copied().unwrap_or(0.0))
        .collect();

    let mut count: f64 = 0.0;
    let mut sum: f64 = 0.0;
    for (row, &c) in centered.iter().enumerate() {
        let include = sets_scale.and_then(|s| s.get(row).copied()).unwrap_or(true);
        if include {
            count += 1.0;
            sum += c;
        }
    }

    let mean = sum / count.max(1.0);
    let mut squares: f64 = 0.0;
    for (row, &c) in centered.iter().enumerate() {
        let include = sets_scale.and_then(|s| s.get(row).copied()).unwrap_or(true);
        if include {
            squares += (c - mean) * (c - mean);
        }
    }

    let deviation = (squares / count.max(1.0)).sqrt().max(0.02);
    centered.iter().map(|&c| (c - mean) / deviation).collect()
}

/// Counts how many hits scored at least `ratio` * top_score. Hits below this are considered "weaker matches".
pub fn strong_count(hits: &[SearchHit], ratio: f64) -> usize {
    match hits.first() {
        Some(top) if top.score > 0.0 => {
            let cutoff = top.score * ratio;
            hits.iter().take_while(|h| h.score >= cutoff).count()
        }
        _ => hits.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terms_and_stopword_removal() {
        let query = "Where is the photo of my cat and dog?";
        let extracted = terms(query);
        // "photo" is in stopwords/kind_words, "the", "of", "my", "and", "where", "is" dropped
        assert_eq!(extracted, vec!["cat", "dog"]);
    }

    #[test]
    fn test_kind_hints_detection() {
        let query = "recent screenshot of error";
        let hints = kind_hints(query);
        assert!(hints.contains(&FileKind::Screenshot));

        let bengali_query = "ভিডিওতে গান";
        let bn_hints = kind_hints(bengali_query);
        assert!(bn_hints.contains(&FileKind::Video));
        assert!(bn_hints.contains(&FileKind::Audio));
    }

    #[test]
    fn test_is_lookup() {
        let lookup_terms = vec!["TRX50".to_string()];
        assert!(is_lookup(&lookup_terms));

        let num_terms = vec!["123456".to_string()];
        assert!(is_lookup(&num_terms));

        let natural_terms = vec!["machine".to_string(), "learning".to_string()];
        assert!(!is_lookup(&natural_terms));
    }

    #[test]
    fn test_meaning_z_score_normalization() {
        // 30 text scores, 30 image scores
        let mut scores = Vec::new();
        let mut modalities = Vec::new();

        for i in 0..30 {
            scores.push(0.5 + (i as f32) * 0.01);
            modalities.push(Modality::Text);
        }
        for i in 0..30 {
            scores.push(0.2 + (i as f32) * 0.01);
            modalities.push(Modality::Image);
        }

        let z = meaning(&scores, &modalities, None);
        assert_eq!(z.len(), 60);

        // Highest score in each modality should have positive z-score
        assert!(z[29] > 0.0);
        assert!(z[59] > 0.0);
        // Lowest should have negative z-score
        assert!(z[0] < 0.0);
        assert!(z[30] < 0.0);
    }

    #[test]
    fn test_strong_count_cutoff() {
        let make_hit = |score: f64| SearchHit {
            path: "/path".to_string(),
            kind: FileKind::Doc,
            score,
            z: Some(score),
            cosine: Some(0.8),
            keyword_rank: Some(1),
            segment: nori_core::types::SegmentKind::Chunk,
            segment_id: 1,
            loc: None,
            loc_end: None,
            excerpt: None,
            modified: 0.0,
            size: 100,
            info: HashMap::new(),
            focus: None,
        };

        let hits = vec![
            make_hit(10.0),
            make_hit(8.0),
            make_hit(5.0),
            make_hit(4.9), // below 0.5 * 10.0
            make_hit(1.0),
        ];

        assert_eq!(strong_count(&hits, 0.5), 3);
    }
}
