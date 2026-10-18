use crate::matches::MatchRange;
use crate::normalize::SearchText;

pub struct Landmark;

impl Landmark {
    /// Discovers a query word on `page` (1-based index) that does not appear on any earlier page (1..page-1).
    ///
    /// When launching a PDF in external viewers (Preview, Acrobat), searching for this unique landmark word
    /// causes the viewer to navigate straight to the target page as its very first hit.
    ///
    /// Returns `None` for page 1 (which opens by default) or if none of the query words appear on the page.
    pub fn find(page: usize, page_texts: &[&str], words: &[&str]) -> Option<String> {
        // Page 1 opens by default, and page must be within bounds
        if page <= 1 || page_texts.len() < page {
            return None;
        }

        let target_text = page_texts[page - 1];

        // Normalized lowercase texts of all earlier pages (0..page-1)
        let earlier_pages: Vec<String> = page_texts[..page - 1]
            .iter()
            .map(|t| SearchText::normalized(t).to_lowercase())
            .collect();

        // Find candidate words with length >= 3 that appear on the target page
        let mut on_page_words: Vec<String> = Vec::new();
        for &word in words {
            if word.chars().count() >= 3 {
                let hits = SearchText::matches(word, target_text);
                if let Some(first_hit) = hits.first() {
                    on_page_words.push(target_text[first_hit.as_range()].to_string());
                }
            }
        }

        // Preference 1: A word that did NOT appear on any earlier page
        for word in &on_page_words {
            let norm_word = SearchText::normalized(word).to_lowercase();
            let appears_earlier = earlier_pages.iter().any(|p| p.contains(&norm_word));
            if !appears_earlier {
                return Some(word.clone());
            }
        }

        // Preference 2: Fall back to the first query word that appears on this page
        on_page_words.into_iter().next()
    }
}

impl SearchText {
    /// Formats a ~`words`-word excerpt around the first matching term, with matched terms enclosed in « ».
    ///
    /// Returns `None` if none of the terms appear in `text`.
    pub fn snippet(text: &str, terms: &[&str], words: usize) -> Option<String> {
        let mut all_matches: Vec<MatchRange> = Vec::new();
        for &term in terms {
            all_matches.extend(Self::matches(term, text));
        }

        if all_matches.is_empty() {
            return None;
        }

        // Sort matches by starting byte offset
        all_matches.sort_by_key(|m| m.start);

        let first = all_matches[0];
        let words_before_target = words / 3;

        // Step 1: Scan backward for ~words/3 words before the first match
        let mut start_byte = first.start;
        let mut before_count = 0usize;

        while start_byte > 0 && before_count < words_before_target {
            let prev_char_idx = text[..start_byte]
                .char_indices()
                .next_back()
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            let prev_char = text[prev_char_idx..start_byte]
                .chars()
                .next()
                .unwrap_or(' ');

            if prev_char.is_whitespace() && start_byte < first.start {
                let next_char = text[start_byte..].chars().next().unwrap_or(' ');
                if !next_char.is_whitespace() {
                    before_count += 1;
                }
            }
            start_byte = prev_char_idx;
        }

        // Trim leading whitespace from start_byte
        while start_byte < first.start
            && text[start_byte..]
                .chars()
                .next()
                .map_or(false, |c| c.is_whitespace())
        {
            start_byte += text[start_byte..].chars().next().unwrap().len_utf8();
        }

        // Step 2: Scan forward for remaining words after the first match
        let words_after_target = words.saturating_sub(before_count);
        let mut end_byte = first.end;
        let mut after_count = 0usize;

        while end_byte < text.len() && after_count < words_after_target {
            let cur_char = text[end_byte..].chars().next().unwrap_or(' ');
            if cur_char.is_whitespace() {
                after_count += 1;
            }
            end_byte += cur_char.len_utf8();
        }

        // Step 3: Construct snippet with « » highlight markers
        let mut out = String::new();
        if start_byte > 0 {
            out.push('…');
        }

        let mut cursor = start_byte;
        for mark in &all_matches {
            if mark.start >= start_byte && mark.end <= end_byte && mark.start >= cursor {
                out.push_str(&text[cursor..mark.start]);
                out.push('«');
                out.push_str(&text[mark.as_range()]);
                out.push('»');
                cursor = mark.end;
            }
        }

        if cursor < end_byte {
            out.push_str(&text[cursor..end_byte]);
        }

        if end_byte < text.len() {
            out.push('…');
        }

        // Step 4: Normalize multiple spaces
        let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(collapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_generation_and_markers() {
        let text = "In the heart of the city, an autonomous system discovered a novel algorithm for vector indexing.";
        let excerpt = SearchText::snippet(text, &["autonomous", "algorithm"], 12)
            .expect("Snippet should exist");

        // Must highlight terms with « »
        assert!(excerpt.contains("«autonomous»"));
        assert!(excerpt.contains("«algorithm»"));
    }

    #[test]
    fn test_snippet_no_match() {
        let text = "A completely unrelated document about gardening and flowers.";
        let excerpt = SearchText::snippet(text, &["database"], 12);
        assert_eq!(excerpt, None);
    }

    #[test]
    fn test_landmark_find_unique_page() {
        let page1 = "Introduction to database systems and relational tables.";
        let page2 = "Detailed analysis of indexing algorithms and query plans.";
        let page3 = "Benchmarking quantum hardware and cryptographic key distribution.";

        let pages = [page1, page2, page3];
        let query_words = ["relational", "quantum", "indexing"];

        // On Page 3, "quantum" is unique to page 3 (did not appear on page 1 or 2)
        let landmark = Landmark::find(3, &pages, &query_words);
        assert_eq!(landmark, Some("quantum".to_string()));

        // Page 1 returns None because it opens to page 1 by default
        assert_eq!(Landmark::find(1, &pages, &query_words), None);
    }
}
