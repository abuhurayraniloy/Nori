use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::params;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::extractors::extract_content;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_path: String,
    pub chunk_text: String,
    pub score: f32,
}

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

    pub fn index_file(
        &self,
        conn: &mut rusqlite::Connection,
        file_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let content = extract_content(file_path)?;
        let text = content.text.trim();
        if text.is_empty() {
            return Ok(());
        }

        let chunks = chunk_by_paragraphs(text, 512);
        let embedding = self.content_embedding(chunks.clone())?;

        Self::save_chunks_to_db(conn, file_path, chunks, embedding)?;
        Ok(())
    }

    pub fn search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let query_embeddings = self.content_embedding(vec![query.to_string()])?;
        let query_vec = &query_embeddings[0];

        let mut stmt =
            conn.prepare("SELECT file_path, chunk_text, embedding FROM document_chunks")?;

        let mut result = Vec::new();

        let rows = stmt.query_map([], |row| {
            let file_path: String = row.get(0)?;
            let chunk_text: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            Ok((file_path, chunk_text, blob))
        })?;

        for row in rows {
            let (file_path, chunk_text, blob) = row?;
            let chunk_vec: &[f32] = bytemuck::cast_slice(&blob);
            let score = cosine_similarity(query_vec, chunk_vec);

            result.push(SearchResult {
                file_path,
                chunk_text,
                score,
            });
        }

        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result.truncate(top_k);

        Ok(result)
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

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (&x, &y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = (norm_a * norm_b).sqrt();

    if denom == 0.0 {
        return 0.0;
    }

    dot / denom
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
        use crate::db::init_db;
        use std::path::Path;

        // 1. Connect to our test SQLite database
        let mut conn = init_db().expect("Failed to initialize database");

        // 2. Initialize Embedder
        let embedder = Embedder::new().expect("Failed to initialize embedder");

        // 3. Pick a real PDF file
        let pdf_path = Path::new(r"D:\pdf\AI Engineering by Chip Huyen.pdf");

        // 4. Run the full indexing pipeline!
        println!("\n🚀 Running index_file pipeline...");
        embedder
            .index_file(&mut conn, pdf_path)
            .expect("index_file failed");

        // 5. Verify data actually landed in SQLite!
        let path_str = pdf_path.to_str().unwrap();

        // Count how many chunks were saved
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM document_chunks WHERE file_path = ?1",
                rusqlite::params![path_str],
                |row| row.get(0),
            )
            .expect("Query failed");

        println!("✅ Successfully saved {} chunks to SQLite!", count);
        assert!(count > 0, "No chunks were saved to the database!");

        // 6. Inspect one saved vector blob from SQLite
        let (sample_text, sample_blob): (String, Vec<u8>) = conn
            .query_row(
                "SELECT chunk_text, embedding FROM document_chunks WHERE file_path = ?1 LIMIT 1",
                rusqlite::params![path_str],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("Failed to retrieve sample row");

        println!("\n--- Sample Saved Chunk Text ---");
        println!("{}...", &sample_text.chars().take(120).collect::<String>());

        println!("\n--- Sample Saved Vector Blob ---");
        println!(
            "Blob byte length: {} bytes (Expected: 768 * 4 = 3072 bytes)",
            sample_blob.len()
        );

        // Verify exact byte size: 768 floats * 4 bytes per float = 3072 bytes
        assert_eq!(sample_blob.len(), 768 * 4);
    }

    #[test]
    fn test_semantic_search() {
        use crate::db::init_db;

        let conn = init_db().expect("Failed to connect to database");
        let embedder = Embedder::new().expect("Failed to initialize embedder");

        // Ask a conceptual question (semantic search!)
        let query = "machine learning deployment and model evaluation";
        println!("\n🔍 Searching for: \"{}\"...", query);

        // Search the top 3 matches
        let results = embedder.search(&conn, query, 3).expect("Search failed");

        println!("==================== SEARCH RESULTS ====================");
        for (i, res) in results.iter().enumerate() {
            println!(
                "\n🏆 Match #{}: Score = {:.2}% ({:.4})",
                i + 1,
                res.score * 100.0,
                res.score
            );
            println!("📁 File: {}", res.file_path);
            println!("📝 Snippet:\n{}", res.chunk_text.trim());
            println!("--------------------------------------------------------");
        }
        println!("========================================================\n");

        assert!(
            !results.is_empty(),
            "Search should return at least one result"
        );
        assert!(results[0].score > 0.0, "Top score should be positive");
    }

    #[test]
    fn benchmark_search_latency() {
        use crate::db::init_db;
        use std::time::Instant;

        let conn = init_db().expect("Failed to connect to database");
        let embedder = Embedder::new().expect("Failed to initialize embedder");

        let query = "machine learning deployment and model evaluation";
        let top_k = 3;

        // --- Phase 1: Measure Query Embedding Time ---
        let start_embed = Instant::now();
        let query_embeddings = embedder
            .content_embedding(vec![query.to_string()])
            .expect("Embedding query failed");
        let query_vec = &query_embeddings[0];
        let embed_duration = start_embed.elapsed();

        // --- Phase 2: Measure SQLite Scan + Cosine Math (Running 100 times for accurate average) ---
        let iterations = 100;
        let mut total_scan_chunks = 0;

        let start_scan = Instant::now();
        for _ in 0..iterations {
            let mut stmt = conn
                .prepare("SELECT file_path, chunk_text, embedding FROM document_chunks")
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                })
                .unwrap();

            let mut results = Vec::new();
            for row in rows {
                let (file_path, chunk_text, blob) = row.unwrap();
                let chunk_vec: &[f32] = bytemuck::cast_slice(&blob);
                let score = cosine_similarity(query_vec, chunk_vec);
                results.push((file_path, chunk_text, score));
            }
            total_scan_chunks = results.len();

            results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            results.truncate(top_k);
        }
        let scan_duration = start_scan.elapsed() / iterations;

        let total_search_time = embed_duration + scan_duration;

        println!("\n================== SEARCH LATENCY REPORT ==================");
        println!("Total chunks scanned in SQLite: {}", total_scan_chunks);
        println!("-----------------------------------------------------------");
        println!(
            "Phase 1: Query Embedding (FastEmbed neural net): {:?} ({:.2} ms)",
            embed_duration,
            embed_duration.as_micros() as f64 / 1000.0
        );
        println!(
            "Phase 2: Database Scan + Cosine Math (SQLite -> RAM): {:?} ({:.2} ms)",
            scan_duration,
            scan_duration.as_micros() as f64 / 1000.0
        );
        println!("-----------------------------------------------------------");
        println!(
            "⚡ TOTAL SEARCH LATENCY:                          {:?} ({:.2} ms)",
            total_search_time,
            total_search_time.as_micros() as f64 / 1000.0
        );
        println!("===========================================================\n");

        assert!(
            total_scan_chunks > 0,
            "No chunks found in database to benchmark!"
        );
    }
}
