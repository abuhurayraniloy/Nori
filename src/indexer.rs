use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::params;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::extractors::extract_content;

pub struct Embedder {
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

    pub fn content_embedding(
        &self,
        chunks: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        let mut model = self.model.lock().map_err(|e| e.to_string())?;
        let embeddings = model.embed(chunks, None)?;
        Ok(embeddings)
    }

    pub fn save_chunks_to_db(
        conn: &mut rusqlite::Connection,
        file_path: &Path,
        chunks: Vec<String>,
        embeddings: Vec<Vec<f32>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tx = conn.transaction()?;

        let path_str = file_path.to_str().expect("Path is invalid");

        tx.execute(
            "DELETE FROM document_chunks
            WHERE file_path = ?1",
            params![path_str],
        )?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO document_chunks (file_path, chunk_index, chunk_text, embedding) VALUES (?1, ?2, ?3, ?4)"
            )?;

            for (idx, (chunk, vector)) in chunks.into_iter().zip(embeddings).enumerate() {
                let vector_bytes: &[u8] = bytemuck::cast_slice(&vector);
                stmt.execute(params![path_str, idx as i64, chunk, vector_bytes])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn index_file(&self, conn: &mut rusqlite::Connection, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = extract_content(file_path)?;
        let text = content.text.trim();
        if text.is_empty() {
            return Ok(())
        }

        let chunks = chunk_by_paragraphs(text, 512);
        let embedding = self.content_embedding(chunks.clone())?;

        Self::save_chunks_to_db(conn, file_path, chunks, embedding)?;
        Ok(())
    }
}

pub fn chunk_by_paragraphs(text: &str, max_chars: usize) -> Vec<String> {
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
        use crate::extractors::extract_content;
        use std::path::Path;

        let path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");

        let document = extract_content(path).expect("Failed to extract pdf content");

        let max_chars = 512;
        let chunks = chunk_by_paragraphs(&document.text, max_chars);

        println!("Total chunks: {}", chunks.len());

        for (i, chunk) in chunks.iter().enumerate() {
            println!(
                "\nChunk: #{} (Length: {}) \nChunk:\n{}\n",
                i + 1,
                chunk.len(),
                chunk
            );
        }
        assert!(!chunks.is_empty(), "Should generate at least one chunk")
    }

    #[test]
    fn test_embedder() {
        use crate::extractors::extract_content;
        use std::path::Path;

        let path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");
        let document = extract_content(path).expect("Failed to extract pdf content");
        let max_chars = 512;
        let chunks = chunk_by_paragraphs(&document.text, max_chars);
        let total_chunks = chunks.len();
        let embedder = Embedder::new().expect("Failed to initialize the embedder");
        let embeddings = embedder
            .content_embedding(chunks)
            .expect("Failed to embedd");

        assert_eq!(embeddings.len(), total_chunks);
    }

    #[test]
    fn test_index_file_end_to_end() {
        use std::path::Path;
        use crate::db::init_db;

        // 1. Connect to our test SQLite database
        let mut conn = init_db().expect("Failed to initialize database");

        // 2. Initialize Embedder
        let embedder = Embedder::new().expect("Failed to initialize embedder");

        // 3. Pick a real PDF file
        let pdf_path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");

        // 4. Run the full indexing pipeline!
        println!("\n🚀 Running index_file pipeline...");
        embedder.index_file(&mut conn, pdf_path).expect("index_file failed");

        // 5. Verify data actually landed in SQLite!
        let path_str = pdf_path.to_str().unwrap();
        
        // Count how many chunks were saved
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM document_chunks WHERE file_path = ?1",
            rusqlite::params![path_str],
            |row| row.get(0),
        ).expect("Query failed");

        println!("✅ Successfully saved {} chunks to SQLite!", count);
        assert!(count > 0, "No chunks were saved to the database!");

        // 6. Inspect one saved vector blob from SQLite
        let (sample_text, sample_blob): (String, Vec<u8>) = conn.query_row(
            "SELECT chunk_text, embedding FROM document_chunks WHERE file_path = ?1 LIMIT 1",
            rusqlite::params![path_str],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).expect("Failed to retrieve sample row");

        println!("\n--- Sample Saved Chunk Text ---");
        println!("{}...", &sample_text.chars().take(120).collect::<String>());

        println!("\n--- Sample Saved Vector Blob ---");
        println!("Blob byte length: {} bytes (Expected: 768 * 4 = 3072 bytes)", sample_blob.len());
        
        // Verify exact byte size: 768 floats * 4 bytes per float = 3072 bytes
        assert_eq!(sample_blob.len(), 768 * 4);
    }

}
