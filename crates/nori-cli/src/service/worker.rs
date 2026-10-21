//! Embedding worker daemon service.

use nori_infer::{Embedder, MockEmbedder};
use serde_json::Value;
use std::io::{BufRead, Write};

pub struct WorkerService {
    text_only: bool,
}

impl WorkerService {
    pub fn new(text_only: bool) -> Self {
        Self { text_only }
    }

    /// Run the JSON-lines embedding worker loop on stdin/stdout.
    pub fn run<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> std::io::Result<()> {
        let mut embedder = MockEmbedder::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                if let Some(op) = val.get("op").and_then(|v| v.as_str()) {
                    match op {
                        "embed" => {
                            let text = val.get("text").and_then(|v| v.as_str()).unwrap_or_default();
                            let vec = embedder
                                .embed_texts(&[text.to_string()])
                                .ok()
                                .and_then(|mut vs| vs.pop())
                                .unwrap_or_default();
                            let resp = serde_json::json!({
                                "ok": true,
                                "vector": vec,
                            });
                            writeln!(writer, "{}", resp)?;
                            writer.flush()?;
                        }
                        "status" => {
                            let resp = serde_json::json!({
                                "ok": true,
                                "text_only": self.text_only,
                                "ready": true,
                            });
                            writeln!(writer, "{}", resp)?;
                            writer.flush()?;
                        }
                        "quit" => {
                            let resp = serde_json::json!({ "ok": true });
                            writeln!(writer, "{}", resp)?;
                            writer.flush()?;
                            break;
                        }
                        _ => {
                            let resp = serde_json::json!({ "error": "unknown op" });
                            writeln!(writer, "{}", resp)?;
                            writer.flush()?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
