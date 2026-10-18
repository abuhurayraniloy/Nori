use unicode_normalization::UnicodeNormalization;

/// Text normalization, linguistic filtering, and lexical filtering.
pub struct SearchText;

impl SearchText {
    /// Normalizes text for keyword search and coordinate folding:
    /// - Decomposes via Unicode NFKD.
    /// - Strips Latin combining diacritics (acute, grave, umlaut, tilde, cedilla).
    /// - Converts zero-width spaces (0x200B) into word break spaces.
    /// - Strips format invisibles (ZWNJ, ZWJ, soft hyphens, BOM, direction marks).
    /// - Normalizes Arabic (strips tatweel/harakat, unifies alef variants).
    /// - Preserves Indic vowel signs and consonants intact.
    pub fn normalized(text: &str) -> String {
        // Step 1: NFKD decomposition (splits accented characters like 'é' into 'e' + U+0301)
        let nfkd_str: String = text.nfkd().collect();

        // Step 2: Custom scalar filtering
        let mut result = String::with_capacity(nfkd_str.len());

        for c in nfkd_str.chars() {
            let val = c as u32;
            match val {
                // Zero-width space: word break in Thai, Khmer, Lao -> standard space
                0x200B => result.push(' '),

                // Combining diacritical marks (strip Latin accents: acute, grave, umlaut, etc.)
                0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF => continue,

                // Formatting invisibles: soft hyphen, BOM, direction marks, joiners
                0x00AD
                | 0x061C
                | 0x200C..=0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x2064
                | 0x2066..=0x2069
                | 0xFEFF => continue,

                // Arabic tatweel (kashida 0x0640) and short vowel marks (harakat 0x064B..=0x065F, 0x0670)
                0x0640 | 0x064B..=0x065F | 0x0670 => continue,

                // Collapse all Arabic alef variants (madda, hamza above, hamza below, wasla) -> plain alef
                0x0622 | 0x0623 | 0x0625 | 0x0671 => result.push('\u{0627}'),

                // Bengali composite vowel: if two-part ে + া appeared, keep canonical
                _ => result.push(c),
            }
        }

        result
    }

    /// Returns true if the word belongs to an inflected/agglutinative script (Indic or Arabic).
    ///
    /// These languages attach suffixes and case endings directly to words (e.g. Bengali "বাংলা" -> "বাংলায়").
    /// In keyword search, such words trigger prefix matching.
    pub fn inflects(word: &str) -> bool {
        word.chars().any(|c| {
            let val = c as u32;
            // Indic: 0x0900..=0x0DFF, Arabic: 0x0600..=0x06FF
            (0x0900..=0x0DFF).contains(&val) || (0x0600..=0x06FF).contains(&val)
        })
    }

    /// Validates whether the text contains natural language rather than pure numerical data dumps.
    ///
    /// Requires at least 20% of non-whitespace characters to be alphabetic letters (`letters * 4 >= others`).
    /// Discards float tables, CSV dumps, and hex logs from wasting 768-dim neural embeddings.
    pub fn has_words(text: &str) -> bool {
        let mut letters = 0usize;
        let mut others = 0usize;

        for c in text.chars() {
            if c.is_whitespace() {
                continue;
            }
            // In Rust, marks count as letters for Indic vowel signs
            let val = c as u32;
            let is_indic_mark = (0x0900..=0x0DFF).contains(&val);
            if c.is_alphabetic() || is_indic_mark {
                letters += 1;
            } else {
                others += 1;
            }
        }

        letters > 0 && letters * 4 >= others
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization_invisibles_and_alef() {
        // Soft hyphen (0x00AD), BOM (0xFEFF), and ZWNJ (0x200C) removal
        let input = "Hello\u{00AD}World\u{FEFF}!\u{200C}";
        assert_eq!(SearchText::normalized(input), "HelloWorld!");

        // Zero-width space becomes normal space
        let thai_like = "WordA\u{200B}WordB";
        assert_eq!(SearchText::normalized(thai_like), "WordA WordB");

        // Arabic alef unification & tatweel stripping
        // أ (0x0623) -> ا (0x0627), ـ (tatweel 0x0640) dropped
        let arabic = "أحمدـ";
        assert_eq!(SearchText::normalized(arabic), "احمد");
    }

    #[test]
    fn test_inflects() {
        assert!(SearchText::inflects("বাংলা")); // Bengali
        assert!(SearchText::inflects("كتاب")); // Arabic
        assert!(!SearchText::inflects("English"));
    }

    #[test]
    fn test_has_words() {
        assert!(SearchText::has_words(
            "This is a document about machine learning."
        ));
        // Number table / CSV dump has 0% letters -> rejected
        assert!(!SearchText::has_words(
            "12.435, 98.231, 104.552, 452.119, 0.002"
        ));
    }
}
