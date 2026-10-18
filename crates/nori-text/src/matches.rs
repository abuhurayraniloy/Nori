use crate::normalize::SearchText;
use unicode_segmentation::UnicodeSegmentation;

/// A byte range in the original source text referencing a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRange {
    pub start: usize,
    pub end: usize,
}

impl MatchRange {
    #[inline]
    pub fn as_range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    #[inline]
    pub fn overlaps(&self, other: &MatchRange) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Returns true if the character belongs to a word (letters, digits, Indic combining signs, nuktas, viramas).
#[inline]
pub fn is_word_character(c: char) -> bool {
    if c.is_alphanumeric() {
        return true;
    }
    let val = c as u32;
    matches!(
        val,
        // Bengali & Devanagari combining marks (nuktas, vowel signs, viramas)
        0x0901..=0x0903 | 0x093C | 0x093E..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963 |
        0x0981..=0x0983 | 0x09BC | 0x09BE..=0x09CD | 0x09D7 | 0x09E2..=0x09E3 |
        // Arabic marks and vowels
        0x0610..=0x061A | 0x064B..=0x065F | 0x0670
    )
}

/// Reverse coordinate tracker: maps positions in normalized text back to original UTF-8 byte ranges.
struct Folded<'a> {
    source: &'a str,
    folded_text: String,
    /// Maps each char index in `folded_text` to `(byte_start, byte_end)` in `source`.
    origin: Vec<(usize, usize)>,
}

impl<'a> Folded<'a> {
    fn new(source: &'a str) -> Self {
        let mut folded_text = String::with_capacity(source.len());
        let mut origin = Vec::with_capacity(source.len());

        // Process by grapheme cluster so multi-scalar letters stay aligned
        let mut byte_offset = 0usize;
        for grapheme in source.graphemes(true) {
            let grapheme_len = grapheme.len();
            let orig_range = (byte_offset, byte_offset + grapheme_len);

            let piece = SearchText::normalized(grapheme).to_lowercase();
            for folded_char in piece.chars() {
                folded_text.push(folded_char);
                origin.push(orig_range);
            }

            byte_offset += grapheme_len;
        }

        Self {
            source,
            folded_text,
            origin,
        }
    }

    /// Converts a character index range in `folded_text` back to byte offsets in `source`.
    fn original_range(&self, start_char: usize, end_char: usize) -> Option<MatchRange> {
        if start_char >= end_char || end_char > self.origin.len() {
            return None;
        }
        let (start_byte, _) = self.origin[start_char];
        let (_, end_byte) = self.origin[end_char - 1];

        if start_byte <= end_byte && end_byte <= self.source.len() {
            debug_assert!(self.source.is_char_boundary(start_byte));
            debug_assert!(self.source.is_char_boundary(end_byte));
            Some(MatchRange {
                start: start_byte,
                end: end_byte,
            })
        } else {
            None
        }
    }
}

impl SearchText {
    /// Finds all keyword match ranges of `word` in `text` (normalized, case-insensitive, diacritic-insensitive),
    /// returned as exact byte ranges in the original verbatim `text`.
    pub fn matches(word: &str, text: &str) -> Vec<MatchRange> {
        let needle = Self::normalized(word).to_lowercase();
        if needle.is_empty() || text.is_empty() {
            return Vec::new();
        }

        let folded = Folded::new(text);
        let prefix = Self::inflects(word);
        let needle_chars_len = needle.chars().count();

        let mut found: Vec<MatchRange> = Vec::new();
        let folded_chars: Vec<char> = folded.folded_text.chars().collect();
        let total_folded = folded_chars.len();

        let mut char_idx = 0usize;
        while char_idx + needle_chars_len <= total_folded && found.len() < 500 {
            // Check substring match in folded text
            let window: String = folded_chars[char_idx..char_idx + needle_chars_len]
                .iter()
                .collect();
            if window == needle {
                let match_start_char = char_idx;
                let match_end_char = char_idx + needle_chars_len;

                if let Some(mut range) = folded.original_range(match_start_char, match_end_char) {
                    // Left boundary check
                    let left_valid = if range.start > 0 {
                        let prev_char = text[..range.start].chars().next_back().unwrap_or(' ');
                        !is_word_character(prev_char)
                    } else {
                        true
                    };

                    if left_valid {
                        // Right boundary check
                        let next_char_opt = text[range.end..].chars().next();
                        let mut keep = true;

                        if next_char_opt.is_some_and(is_word_character) {
                            if prefix {
                                // For inflected scripts (Bengali, Arabic), swallow the attached ending
                                let mut extended_end = range.end;
                                for trailing_char in text[range.end..].chars() {
                                    if is_word_character(trailing_char) {
                                        extended_end += trailing_char.len_utf8();
                                    } else {
                                        break;
                                    }
                                }
                                range.end = extended_end;
                            } else {
                                keep = false;
                            }
                        }

                        if keep {
                            if let Some(last) = found.last() {
                                if !last.overlaps(&range) {
                                    found.push(range);
                                }
                            } else {
                                found.push(range);
                            }
                        }
                    }
                }

                char_idx += needle_chars_len;
            } else {
                char_idx += 1;
            }
        }

        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbatim_slice_with_accents_and_case() {
        let text = "Here is a Café with good coffee.";
        let hits = SearchText::matches("cafe", text);
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].as_range()], "Café");
    }

    #[test]
    fn test_verbatim_slice_with_soft_hyphen() {
        let text = "Read this docu\u{00AD}ment carefully.";
        let hits = SearchText::matches("document", text);
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].as_range()], "docu\u{00AD}ment");
    }

    #[test]
    fn test_word_boundary_avoids_substrings() {
        let text = "The cat scattered across the catering floor.";
        let hits = SearchText::matches("cat", text);
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].as_range()], "cat");
    }

    #[test]
    fn test_inflected_script_prefix_swallow() {
        let text = "আমাদের বাংলায় অনেক সুন্দর জায়গা আছে।";
        let hits = SearchText::matches("বাংলা", text);
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].as_range()], "বাংলায়");
    }

    #[test]
    fn test_swift_parity_arabic_and_bengali() {
        let arabic = "زرنا مدينة البتراء الأثرية في جنوب الأردن.";
        let hits_ar = SearchText::matches("الاردن", arabic);
        assert_eq!(hits_ar.len(), 1);
        assert_eq!(&arabic[hits_ar[0].as_range()], "الأردن");

        let bengali = "একটি বড় কুমিরের ছবি";
        let hits_bn = SearchText::matches("কুমির", bengali);
        assert_eq!(hits_bn.len(), 1);
        assert_eq!(&bengali[hits_bn[0].as_range()], "কুমিরের");
    }

    #[test]
    fn test_negative_guards_no_overstripping() {
        // "বা" must not match plain "ব"
        let text = "বইটি ভালো";
        let hits = SearchText::matches("বা", text);
        assert!(hits.is_empty());
    }
}
