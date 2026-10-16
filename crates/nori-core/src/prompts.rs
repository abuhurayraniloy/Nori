/// DeepMind EmbeddingGemma 2 task prompt formatters.
///
/// Images, audio, and video inputs take no text prompt.

pub struct Prompts;

impl Prompts {
    /// Standard natural language query prompt for standard document search.
    #[inline]
    pub fn query(text: &str) -> String {
        format!("task: search result | query: {}", text)
    }

    #[inline]
    pub fn code_query(text: &str) -> String {
        format!("task: code retrieval | query: {}", text)
    }

    #[inline]
    pub fn document(title: Option<&str>, text: &str) -> String {
        let title_str = match title {
            Some(t) if !t.trim().is_empty() => t,
            _ => "none",
        };
        format!("title: {} | text: {}", title_str, text)
    }
}

pub struct CodeQuery;

impl CodeQuery {
    pub const PREFIX: &'static str = "code:";

    pub fn parse(query: &str) -> Option<&str> {
        let trimmed = query.trim_start();

        if trimmed.len() >= Self::PREFIX.len()
            && trimmed[..Self::PREFIX.len()].eq_ignore_ascii_case(Self::PREFIX)
        {
            Some(trimmed[Self::PREFIX.len()..].trim())
        } else {
            None
        }
    }
}
