use std::sync::{Arc, Mutex};
use fastembed::{EmbeddingModel, TextInitOptions, TextEmbedding};

struct Embedder {
    model: Arc<Mutex<TextEmbedding>>,
}

impl Embedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let options = TextInitOptions::new(EmbeddingModel::SnowflakeArcticEmbedM)
            .with_show_download_progress(true)
            .with_max_length(512)
            .with_intra_threads(4);

        let model = Arc::new(Mutex::new(TextEmbedding::try_new(options)?));
        Ok(Self { model })
    }

    pub fn content_embedding(&self, chunks: Vec<String>) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        let mut model = self.model.lock()
            .map_err(|e| e.to_string())?;
        let embeddings = model.embed(chunks, None)?;
        Ok(embeddings)
    }
}

fn chunk_by_paragraphs(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunked = Vec::new();
    let mut chunk = String::new();
    for paragraph in text.split("\n\n") {
        let p = paragraph.trim();
        if p.is_empty() {
            continue;
        }

        if chunk.len() + p.len() > max_chars && !chunk.is_empty() {
            chunked.push(std::mem::take(&mut chunk));
        }

        if !chunk.is_empty() {
            chunk.push_str("\n\n");
        }

        chunk.push_str(p);
    }

    if !chunk.is_empty() {
        chunked.push(chunk);
    }
    chunked
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_embedding_models() {
        use fastembed::TextEmbedding;
        dbg!(TextEmbedding::list_supported_models());
        assert_eq!(true, true);
    }
    
    #[test]
    fn test_chunk_text_by_paragraphs() {
        use std::path::Path;
        use crate::extractors::extract_content;

        let path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");

        let document = extract_content(path).expect("Failed to extract pdf content");

        // println!("Extracted text:\n {}", document.text);

        let max_chars = 600;
        let chunks = chunk_by_paragraphs(&document.text, max_chars);

        println!("Total chunks: {}", chunks.len());

        for (i, chunk) in chunks.iter().enumerate() {
            println!("\nChunk: #{} (Length: {}) \nChunk:\n{}\n", i + 1, chunk.len(), chunk);
        }
         assert!(!chunks.is_empty(), "Should generate at least one chunk")
    }

    #[test]
    fn test_embedder() {
        use std::path::Path;
        use crate::extractors::extract_content;

        let path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");
        let document = extract_content(path).expect("Failed to extract pdf content");
        let max_chars = 512;
        let chunks = chunk_by_paragraphs(&document.text, max_chars);
        let total_chunks = chunks.len();
        let embedder = Embedder::new().expect("Failed to initialize the embedder");
        let embeddings = embedder.content_embedding(chunks).expect("Failed to embedd");

        assert_eq!(embeddings.len(), total_chunks);
    }
}

