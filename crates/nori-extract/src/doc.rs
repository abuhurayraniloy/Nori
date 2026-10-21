use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

/// Document extraction errors.
#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unreadable: {0}")]
    Unreadable(String),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Plaintext, HTML, Markdown, RTF, DOCX, and ODT document extractor.
pub struct DocExtractor;

impl DocExtractor {
    /// Extracts plain text from `.txt`, `.md`, `.html`, `.htm`, `.rtf`, `.docx`, `.odt`.
    pub fn text(path: &Path) -> Result<String, ExtractError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "docx" => Self::rich_docx(path),
            "odt" => Self::rich_odt(path),
            "rtf" => {
                let raw = Self::read_text(path)?;
                Ok(RTF::text(&raw))
            }
            "html" | "htm" => {
                let raw = Self::read_text(path)?;
                Ok(Self::plain_text_from_html(&raw))
            }
            "doc" => Err(ExtractError::Unreadable(
                "legacy Word (.doc) files aren't read on this system".to_string(),
            )),
            _ => Self::read_text(path).map_err(ExtractError::Io),
        }
    }

    /// Reads file as UTF-8, falling back to Latin-1 (ISO-8859-1) if UTF-8 fails.
    pub fn read_text(path: &Path) -> Result<String, std::io::Error> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(e) => {
                // ISO-8859-1 fallback: each byte maps 1:1 to Unicode scalar U+0000..U+00FF
                let latin1 = e.into_bytes().into_iter().map(|b| b as char).collect();
                Ok(latin1)
            }
        }
    }

    /// Converts HTML into clean plain text for search indexing.
    ///
    /// Drops scripts, styles, head, and comments; turns block ends into newlines;
    /// strips tags; and decodes standard and numeric HTML entities.
    pub fn plain_text_from_html(html: &str) -> String {
        static SCRIPT_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?is)<script\b.*?</script\s*>").unwrap()
        });
        static STYLE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?is)<style\b.*?</style\s*>").unwrap()
        });
        static COMMENT_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").unwrap());
        static HEAD_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"(?is)<head\b.*?</head\s*>").unwrap());
        static BLOCKS_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?i)<(br|/p|/div|/li|/h[1-6]|/tr|/section|/article)\b[^>]*>")
                .unwrap()
        });
        static TAGS_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
        static SPACES_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"[ \t]+").unwrap());
        static NEWLINES_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"\s*\n\s*").unwrap());

        let mut text = html.to_string();
        text = SCRIPT_RE.replace_all(&text, " ").into_owned();
        text = STYLE_RE.replace_all(&text, " ").into_owned();
        text = COMMENT_RE.replace_all(&text, " ").into_owned();
        text = HEAD_RE.replace_all(&text, " ").into_owned();

        text = BLOCKS_RE.replace_all(&text, "\n").into_owned();
        text = TAGS_RE.replace_all(&text, " ").into_owned();

        let entities: &[(&str, &str)] = &[
            ("&nbsp;", " "),
            ("&amp;", "&"),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&quot;", "\""),
            ("&#39;", "'"),
            ("&apos;", "'"),
            ("&mdash;", "—"),
            ("&ndash;", "–"),
            ("&hellip;", "…"),
        ];

        for (entity, replacement) in entities {
            text = text.replace(entity, replacement);
        }

        text = Self::decode_numeric_entities(&text);
        text = SPACES_RE.replace_all(&text, " ").into_owned();
        text = NEWLINES_RE.replace_all(&text, "\n").into_owned();

        text.trim().to_string()
    }

    /// Decodes numeric entities like `&#233;` (decimal) or `&#x2014;` (hexadecimal).
    fn decode_numeric_entities(text: &str) -> String {
        if !text.contains("&#") {
            return text.to_string();
        }

        static NUM_ENTITY_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"&#(x[0-9a-fA-F]+|[0-9]+);").unwrap()
        });

        NUM_ENTITY_RE
            .replace_all(text, |caps: &regex::Captures| {
                let code_str = &caps[1];
                let scalar = if let Some(hex) = code_str.strip_prefix('x') {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else {
                    code_str.parse::<u32>().ok().and_then(char::from_u32)
                };

                match scalar {
                    Some(ch) => ch.to_string(),
                    None => " ".to_string(),
                }
            })
            .into_owned()
    }

    /// Splits text into ~`size`-character pieces (~450 tokens) that overlap slightly,
    /// breaking strictly at whitespace/word boundaries.
    pub fn chunks(text: &str, size: usize, overlap: usize, limit: usize) -> Vec<String> {
        static SPACE_TAB_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"[ \t\u{00A0}]+").unwrap()
        });
        static MULTI_NEWLINE_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"\n\s*\n\s*\n+").unwrap());

        let clean = SPACE_TAB_RE.replace_all(text, " ");
        let clean = MULTI_NEWLINE_RE.replace_all(&clean, "\n\n");
        let clean = clean.trim();

        if clean.is_empty() {
            return Vec::new();
        }

        let chars: Vec<char> = clean.chars().collect();
        let total_chars = chars.len();
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < total_chars && chunks.len() < limit {
            let mut end = (start + size).min(total_chars);

            if end < total_chars {
                // Look for the last whitespace between start and end
                if let Some(space_offset) = chars[start..end].iter().rposition(|c| c.is_whitespace()) {
                    if space_offset > size / 2 {
                        end = start + space_offset;
                    }
                }
            }

            let chunk_str: String = chars[start..end].iter().collect();
            let trimmed = chunk_str.trim();
            if !trimmed.is_empty() {
                chunks.push(trimmed.to_string());
            }

            if end >= total_chars {
                break;
            }

            let mut next = end.saturating_sub(overlap);
            if next <= start {
                next = end;
            }

            // Start next chunk on a word boundary (skip to after whitespace)
            while next < end && !chars[next].is_whitespace() {
                next += 1;
            }
            while next < end && chars[next].is_whitespace() {
                next += 1;
            }

            start = if next < end { next } else { end };
        }

        chunks
    }

    /// Extracts text from `.docx` via zipped `word/document.xml`.
    fn rich_docx(path: &Path) -> Result<String, ExtractError> {
        let xml = ZipExtractor::entry("word/document.xml", path)?;
        let html = Self::as_html(&xml, &["w:p"], &["w:br", "w:cr"], &["w:tab"]);
        Ok(Self::plain_text_from_html(&html))
    }

    /// Extracts text from `.odt` via zipped `content.xml`.
    fn rich_odt(path: &Path) -> Result<String, ExtractError> {
        let xml = ZipExtractor::entry("content.xml", path)?;
        let html = Self::as_html(
            &xml,
            &["text:p", "text:h"],
            &["text:line-break"],
            &["text:tab", "text:s"],
        );
        Ok(Self::plain_text_from_html(&html))
    }

    /// Converts XML tags to HTML tags recognized by `plain_text_from_html`.
    fn as_html(xml: &str, paragraphs: &[&str], breaks: &[&str], spaces: &[&str]) -> String {
        let mut text = xml.to_string();
        for tag in paragraphs {
            let closing = format!("</{}>", tag);
            text = text.replace(&closing, "</p>");
        }
        for tag in breaks {
            let re = regex::Regex::new(&format!(r"<{}\b[^>]*/?>", regex::escape(tag))).unwrap();
            text = re.replace_all(&text, "<br>").into_owned();
        }
        for tag in spaces {
            let re = regex::Regex::new(&format!(r"<{}\b[^>]*/?>", regex::escape(tag))).unwrap();
            text = re.replace_all(&text, " ").into_owned();
        }
        text
    }
}

/// Generates a single-line excerpt of up to `limit` characters for result listings.
pub fn excerpt(text: &str, limit: usize) -> String {
    static MULTI_SPACE_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());

    // Only clean the beginning of the text to keep performance high for huge documents
    let max_sample_len = limit * 8;
    let sample: String = text.chars().take(max_sample_len).collect();
    let line = MULTI_SPACE_RE.replace_all(&sample, " ").trim().to_string();

    let line_chars: Vec<char> = line.chars().collect();
    if line_chars.len() > limit || sample.chars().count() < text.chars().count() {
        let prefix: String = line_chars.into_iter().take(limit).collect();
        format!("{}…", prefix)
    } else {
        line
    }
}

/// Safe Zip entry reader with uncompressed size limit against zip-bombs.
struct ZipExtractor;

impl ZipExtractor {
    /// 64 MB maximum uncompressed entry size.
    const MAX_ENTRY_BYTES: u64 = 64 << 20;

    fn entry(name: &str, path: &Path) -> Result<String, ExtractError> {
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut entry = archive
            .by_name(name)
            .map_err(|_| ExtractError::Unreadable(format!("no {name} inside")))?;

        if entry.size() > Self::MAX_ENTRY_BYTES {
            return Err(ExtractError::Unreadable(format!(
                "{name} is {} MB, more than is read",
                entry.size() >> 20
            )));
        }

        let mut content = String::with_capacity(entry.size() as usize);
        entry.read_to_string(&mut content)?;
        Ok(content)
    }
}

/// Rich Text Format (.rtf) parser.
pub struct RTF;

impl RTF {
    pub const SKIPPED_GROUPS: &[&str] = &[
        "fonttbl",
        "colortbl",
        "stylesheet",
        "info",
        "pict",
        "header",
        "footer",
        "headerl",
        "headerr",
        "footerl",
        "footerr",
        "listtable",
        "listoverridetable",
        "rsidtbl",
        "generator",
        "themedata",
        "colorschememapping",
        "datastore",
        "xmlnstbl",
        "latentstyles",
        "pgdsctbl",
        "filetbl",
        "revtbl",
        "mmathPr",
        "fldinst",
        "object",
    ];

    /// Extracts clean readable text from raw RTF markup.
    pub fn text(rtf: &str) -> String {
        let chars: Vec<char> = rtf.chars().collect();
        let total = chars.len();
        let mut out = String::new();
        let mut i = 0;
        let mut skip_depth = 0;
        let mut unicode_skip = 1usize;
        let mut pending_skip = 0usize;

        while i < total {
            let c = chars[i];
            if skip_depth > 0 {
                if c == '{' {
                    skip_depth += 1;
                } else if c == '}' {
                    skip_depth -= 1;
                } else if c == '\\' {
                    i += 1;
                }
                i += 1;
                continue;
            }

            match c {
                '{' => {
                    let mut j = i + 1;
                    if j < total && chars[j] == '\\' {
                        j += 1;
                        if j < total && chars[j] == '*' {
                            // {\* ...} is a destination readers may ignore
                            skip_depth = 1;
                            i = j + 1;
                            continue;
                        }
                        let mut word = String::new();
                        while j < total && chars[j].is_ascii_alphabetic() && word.len() < 32 {
                            word.push(chars[j]);
                            j += 1;
                        }
                        if Self::SKIPPED_GROUPS.contains(&word.as_str()) {
                            skip_depth = 1;
                            i = j;
                            continue;
                        }
                    }
                    i += 1;
                }
                '}' => {
                    i += 1;
                }
                '\\' => {
                    i += 1;
                    if i >= total {
                        break;
                    }
                    let next = chars[i];
                    if next == '\'' {
                        // \'hh: hex encoded byte in document code page (Windows-1252)
                        if i + 2 < total {
                            let hex_str: String = chars[i + 1..=i + 2].iter().collect();
                            if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                                if pending_skip > 0 {
                                    pending_skip -= 1;
                                } else {
                                    out.push(Self::windows_1252(byte));
                                }
                                i += 3;
                            } else {
                                i += 1;
                            }
                        } else {
                            i += 1;
                        }
                    } else if next.is_ascii_alphabetic() {
                        let mut word = String::new();
                        let mut number = String::new();
                        let mut j = i;
                        while j < total && chars[j].is_ascii_alphabetic() {
                            word.push(chars[j]);
                            j += 1;
                        }
                        if j < total && chars[j] == '-' {
                            number.push('-');
                            j += 1;
                        }
                        while j < total && chars[j].is_ascii_digit() {
                            number.push(chars[j]);
                            j += 1;
                        }
                        if j < total && chars[j] == ' ' {
                            j += 1; // Trailing space is part of the control word
                        }

                        match word.as_str() {
                            "par" | "line" | "sect" | "page" | "row" => out.push('\n'),
                            "tab" | "cell" => out.push('\t'),
                            "emdash" => out.push('—'),
                            "endash" => out.push('–'),
                            "lquote" => out.push('‘'),
                            "rquote" => out.push('’'),
                            "ldblquote" => out.push('“'),
                            "rdblquote" => out.push('”'),
                            "bullet" => out.push('•'),
                            "uc" => unicode_skip = number.parse::<usize>().unwrap_or(1),
                            "u" => {
                                if let Ok(val) = number.parse::<i32>() {
                                    if (-65536..65536).contains(&val) {
                                        let uval = if val < 0 { val + 65536 } else { val } as u32;
                                        if let Some(scalar) = char::from_u32(uval) {
                                            out.push(scalar);
                                        }
                                    }
                                }
                                pending_skip = unicode_skip;
                            }
                            _ => {}
                        }
                        i = j;
                    } else {
                        match next {
                            '\\' | '{' | '}' => out.push(next),
                            '~' => out.push(' '),
                            '\n' | '\r' => out.push('\n'),
                            _ => {}
                        }
                        i += 1;
                    }
                }
                '\r' | '\n' => {
                    i += 1;
                }
                _ => {
                    if pending_skip > 0 {
                        pending_skip -= 1;
                    } else {
                        out.push(c);
                    }
                    i += 1;
                }
            }
        }

        out
    }

    /// Windows-1252 upper half mapping; remainder is Latin-1.
    fn windows_1252(byte: u8) -> char {
        match byte {
            0x80 => '€',
            0x82 => '‚',
            0x83 => 'ƒ',
            0x84 => '„',
            0x85 => '…',
            0x86 => '†',
            0x87 => '‡',
            0x88 => 'ˆ',
            0x89 => '‰',
            0x8A => 'Š',
            0x8B => '‹',
            0x8C => 'Œ',
            0x8E => 'Ž',
            0x91 => '‘',
            0x92 => '’',
            0x93 => '“',
            0x94 => '”',
            0x95 => '•',
            0x96 => '–',
            0x97 => '—',
            0x98 => '˜',
            0x99 => '™',
            0x9A => 'š',
            0x9B => '›',
            0x9C => 'œ',
            0x9E => 'ž',
            0x9F => 'Ÿ',
            _ => byte as char,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_becomes_plain_text() {
        let html = r#"
            <html><head><title>T</title><style>p { color: red }</style></head>
            <body><h1>Trip &amp; plans</h1><p>Fly to <b>Tokyo</b>&nbsp;on Friday.</p><script>var x = 1;</script>
            <p>Caf&#233; &#x2014; ramen</p><!-- hidden --></body></html>
            "#;

        assert_eq!(
            DocExtractor::plain_text_from_html(html),
            "Trip & plans\nFly to Tokyo on Friday.\nCafé — ramen"
        );
    }

    #[test]
    fn test_chunks_overlap_and_respect_words() {
        let text = (1..=600)
            .map(|i| format!("word{}", i))
            .collect::<Vec<_>>()
            .join(" ");

        let chunks = DocExtractor::chunks(&text, 500, 100, 100);
        assert!(chunks.len() > 5);
        assert!(chunks.iter().all(|c| c.chars().count() <= 500));
        assert!(chunks.iter().all(|c| c.starts_with("word"))); // Never starts mid-word
        assert!(chunks.last().unwrap().ends_with("word600"));
    }

    #[test]
    fn test_rtf_parsing() {
        let rtf = r#"{\rtf1\ansi\deff0 {\fonttbl{\f0 Arial;}}
{\colortbl ;\red255\green0\blue0;}
This is \b bold\b0  text and a bullet: \bullet\par
Second line with \'e9 (caf\'e9) and unicode \u8212? emdash.}"#;

        let parsed = RTF::text(rtf);
        assert!(parsed.contains("This is bold text and a bullet: •"));
        assert!(parsed.contains("café"));
        assert!(parsed.contains("—"));
    }

    #[test]
    fn test_excerpt_truncation() {
        let long_text = "This is a comprehensive overview of machine learning algorithms across multiple pages and sections.";
        let exc = excerpt(long_text, 30);
        assert!(exc.ends_with('…'));
        assert_eq!(exc.chars().count(), 31); // 30 chars + 1 ellipsis
    }
}
