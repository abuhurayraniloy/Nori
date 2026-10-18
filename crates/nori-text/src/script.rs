use std::ops::RangeInclusive;
use unicode_normalization::UnicodeNormalization;

/// Unicode scalar ranges for complex inflected scripts.
pub struct Script;

impl Script {
    /// Indic scripts (Devanagari, Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada, Malayalam).
    pub const INDIC: RangeInclusive<u32> = 0x0900..=0x0DFF;

    /// Arabic script range.
    pub const ARABIC: RangeInclusive<u32> = 0x0600..=0x06FF;
}

/// Identifies PDF text layer corruption that requires visual or OCR fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTextProblem {
    /// Bengali or Hindi extracted in physical glyph order rather than phonological order
    /// (dependent vowel signs precede their consonants, or conjuncts were broken).
    GlyphOrder,

    /// Arabic letters dropped where a font drew a lam-alif ligature, leaving a bare alef
    /// before '?', '.', or the Unicode replacement character (e.g. "المغرب" -> "ا?مغرب").
    LostLetters,
}

impl PageTextProblem {
    /// Inspects extracted page text for structural script corruption.
    pub fn check(text: &str) -> Option<Self> {
        // Step 1: Normalize to Unicode NFC (canonical composition)
        let nfc_text: String = text.nfc().collect();

        let mut vowel_signs = 0usize;
        let mut misplaced = 0usize;
        let mut lost_letters = 0usize;

        let mut previous: char = ' ';
        let mut before_previous: char = ' ';

        for c in nfc_text.chars() {
            let val = c as u32;

            // Check 1: Indic dependent vowel sign misplaced
            if Self::is_indic_vowel_sign(val) {
                vowel_signs += 1;
                if !Self::is_indic_consonant_or_nukta(previous as u32) {
                    misplaced += 1;
                }
            }

            // Check 2: Arabic lam ligature swallowed after bare alef (0x0627)
            // Bare alef followed by '?', '.', or '\u{FFFD}', followed by an Arabic letter
            if before_previous == '\u{0627}'
                && (previous == '?' || previous == '.' || previous == '\u{FFFD}')
                && Script::ARABIC.contains(&val)
                && c.is_alphabetic()
            {
                lost_letters += 1;
            }

            before_previous = previous;
            previous = c;
        }

        // Threshold 1: At least 10 vowel signs and >= 5% misplaced indicates glyph-order rendering
        if vowel_signs >= 10 && misplaced * 20 >= vowel_signs {
            return Some(Self::GlyphOrder);
        }

        // Threshold 2: At least 2 broken lam ligatures indicates lost letters
        if lost_letters >= 2 {
            return Some(Self::LostLetters);
        }

        None
    }

    /// Dependent vowel signs (kar / matras) of Bengali and Devanagari.
    #[inline]
    fn is_indic_vowel_sign(val: u32) -> bool {
        match val {
            // Bengali dependent vowel signs
            0x09BE..=0x09C4 | 0x09C7 | 0x09C8 | 0x09CB | 0x09CC | 0x09D7 | 0x09E2 | 0x09E3 => true,
            // Devanagari dependent vowel signs
            0x093A
            | 0x093B
            | 0x093E..=0x094C
            | 0x094E
            | 0x094F
            | 0x0955..=0x0957
            | 0x0962
            | 0x0963 => true,
            _ => false,
        }
    }

    /// Consonants and nuktas of Bengali and Devanagari.
    #[inline]
    fn is_indic_consonant_or_nukta(val: u32) -> bool {
        match val {
            // Bengali consonants, nukta, khanda ta, additional consonants
            0x0995..=0x09B9 | 0x09BC | 0x09CE | 0x09DC | 0x09DD | 0x09DF | 0x09F0 | 0x09F1 => true,
            // Devanagari consonants, nukta, additional consonants
            0x0915..=0x0939 | 0x093C | 0x0958..=0x095F | 0x0979..=0x097F => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_bengali_text_no_problem() {
        // Valid Bengali: Consonant followed by vowel sign (ক + া = কা)
        let clean = "আমাদের দেশ বাংলাদেশ। এখানে অনেক মানুষ বাস করে। তারা সবাই মিলেমিশে থাকে।";
        assert_eq!(PageTextProblem::check(clean), None);
    }

    #[test]
    fn test_bengali_glyph_order_detected() {
        // Simulating Quartz glyph-order extraction failure:
        // Vowel signs (া, ি, ী, ু, ূ) preceded by spaces or symbols rather than consonants
        let mut corrupted = String::new();
        for _ in 0..12 {
            // Space followed by vowel sign া (0x09BE)
            corrupted.push_str(" \u{09BE}ক");
        }
        assert_eq!(
            PageTextProblem::check(&corrupted),
            Some(PageTextProblem::GlyphOrder)
        );
    }

    #[test]
    fn test_arabic_lost_letters_detected() {
        // Bare alef (ا 0x0627) + '?' + Arabic letter (م 0x0645) occurring twice
        let corrupt_arabic = "هذا ا?مغرب و ا?جزيرة في الأخبار";
        assert_eq!(
            PageTextProblem::check(corrupt_arabic),
            Some(PageTextProblem::LostLetters)
        );
    }

    #[test]
    fn test_arabic_abbreviations_not_flagged() {
        // "أ.د." (Prof. Dr.) uses hamza alef (أ 0x0623), not bare alef (0x0627), so it's not flagged
        let doc_with_abbrev = "الأستاذ أ.د. محمد شارك في المؤتمر";
        assert_eq!(PageTextProblem::check(doc_with_abbrev), None);
    }
}
