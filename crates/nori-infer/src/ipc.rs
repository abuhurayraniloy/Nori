use crate::traits::{Embedder, EmbeddingResult, WorkerInfo};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("worker not found: {0}")]
    NotFound(String),
    #[error("worker error: {0}")]
    Failed(String),
    #[error("worker died: {0}")]
    Died(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Configuration used to spawn an external embedding worker process.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub environment: HashMap<String, String>,
    pub log: Option<PathBuf>,
}

impl WorkerConfig {
    pub fn new<P: Into<PathBuf>>(executable: P, arguments: Vec<String>) -> Self {
        Self {
            executable: executable.into(),
            arguments,
            environment: HashMap::new(),
            log: None,
        }
    }

    /// Bundled helper invocation (`nori worker`).
    pub fn helper<P: Into<PathBuf>>(executable: P) -> Self {
        Self::new(executable, vec!["worker".to_string()])
    }

    /// Python mlx-vlm or PyTorch worker inside a virtual environment.
    pub fn python<P: AsRef<Path>>(folder: P) -> Result<Self, WorkerError> {
        let folder = folder.as_ref();
        // Check Windows path (.venv/Scripts/python.exe) then Unix (.venv/bin/python)
        let win_python = folder.join(".venv").join("Scripts").join("python.exe");
        let unix_python = folder.join(".venv").join("bin").join("python");

        let python_bin = if win_python.exists() {
            win_python
        } else if unix_python.exists() {
            unix_python
        } else {
            return Err(WorkerError::NotFound(format!(
                "worker environment missing in {}; run: uv sync --project {}",
                folder.display(),
                folder.display()
            )));
        };

        let script = folder.join("nori_worker.py");
        let model = "google/embeddinggemma-2";

        let mut config = Self::new(
            python_bin,
            vec![
                "-I".to_string(),
                script.to_string_lossy().to_string(),
                "--model".to_string(),
                model.to_string(),
            ],
        );

        config
            .environment
            .insert("TOKENIZERS_PARALLELISM".to_string(), "false".to_string());

        if is_model_cached(model) {
            config
                .environment
                .insert("HF_HUB_OFFLINE".to_string(), "1".to_string());
        }

        Ok(config)
    }

    /// Reads worker directory from environment variable `NORI_WORKER_DIR`.
    pub fn from_environment() -> Option<Result<Self, WorkerError>> {
        if let Ok(dir) = std::env::var("NORI_WORKER_DIR") {
            return Some(Self::python(dir));
        }
        None
    }
}

/// Checks if Hugging Face model snapshot is cached locally.
pub fn is_model_cached(model: &str) -> bool {
    let path = Path::new(model);
    if path.exists() {
        return true;
    }

    let folder = format!("models--{}", model.replace('/', "--"));
    let hub_cache = std::env::var("HF_HUB_CACHE").ok().map(PathBuf::from).or_else(|| {
        dirs::home_dir().map(|h| h.join(".cache").join("huggingface").join("hub"))
    });

    if let Some(hub) = hub_cache {
        hub.join(folder).join("snapshots").exists()
    } else {
        false
    }
}

/// Fallback dirs crate replacement for home directory resolution.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var("USERPROFILE")
                .ok()
                .map(PathBuf::from)
                .or_else(|| {
                    let drive = std::env::var("HOMEDRIVE").ok()?;
                    let path = std::env::var("HOMEPATH").ok()?;
                    Some(PathBuf::from(format!("{}{}", drive, path)))
                })
        }
        #[cfg(not(windows))]
        {
            std::env::var("HOME").ok().map(PathBuf::from)
        }
    }
}

/// Base64 little-endian 32-bit float encoder.
pub fn encode_f32_vectors(vectors: &[Vec<f32>]) -> String {
    let total_floats: usize = vectors.iter().map(|v| v.len()).sum();
    let mut bytes = Vec::with_capacity(total_floats * 4);
    for v in vectors {
        for &f in v {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    BASE64_STANDARD.encode(&bytes)
}

/// Base64 little-endian 32-bit float decoder into flat float vector chunks.
pub fn decode_f32_vectors(
    encoded: &str,
    count: usize,
    dim: usize,
) -> Result<Vec<Vec<f32>>, WorkerError> {
    let bytes = BASE64_STANDARD
        .decode(encoded.trim())
        .map_err(|e| WorkerError::Failed(format!("invalid base64 float data: {e}")))?;

    let expected = count * dim * 4;
    if bytes.len() != expected {
        return Err(WorkerError::Failed(format!(
            "byte count mismatch: expected {} bytes ({} x {} x 4), got {}",
            expected,
            count,
            dim,
            bytes.len()
        )));
    }

    let mut result = Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(dim * 4) {
        let mut row = Vec::with_capacity(dim);
        for f_bytes in chunk.chunks_exact(4) {
            let val = f32::from_le_bytes(f_bytes.try_into().unwrap());
            row.push(val);
        }
        result.push(row);
    }
    Ok(result)
}

/// Outgoing request payload sent to the worker over stdin.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub id: u64,
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<usize>,
}

/// Incoming reply payload received from the worker over stdout.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerReply {
    pub id: Option<u64>,
    pub ok: Option<bool>,
    pub ready: Option<bool>,
    pub error: Option<String>,
    pub dim: Option<usize>,
    pub count: Option<usize>,
    pub vectors: Option<String>,
    pub errors: Option<Vec<Option<String>>>,
    pub info: Option<WorkerInfo>,
}

/// Communicates with an out-of-process embedding worker over standard pipes using JSON lines.
pub struct EmbeddingWorker {
    pub info: WorkerInfo,
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    stdout_lines: std::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
    log_path: Option<PathBuf>,
}

impl EmbeddingWorker {
    pub fn new(config: WorkerConfig, text_only: bool) -> Result<Self, WorkerError> {
        let mut cmd = Command::new(&config.executable);
        let mut args = config.arguments.clone();
        if text_only && !args.iter().any(|a| a == "--text-only") {
            args.push("--text-only".to_string());
        }
        cmd.args(&args);

        for (k, v) in &config.environment {
            cmd.env(k, v);
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());

        if let Some(ref log_path) = config.log {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)?;
            cmd.stderr(Stdio::from(file));
        } else {
            cmd.stderr(Stdio::null());
        }

        let mut child = cmd.spawn().map_err(|e| {
            WorkerError::NotFound(format!(
                "failed to spawn worker '{}': {e}",
                config.executable.display()
            ))
        })?;

        let stdin = BufWriter::new(child.stdin.take().ok_or_else(|| {
            WorkerError::Failed("failed to open stdin pipe for worker".to_string())
        })?);

        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| {
            WorkerError::Failed("failed to open stdout pipe for worker".to_string())
        })?);

        let mut stdout_lines = stdout.lines();

        // Wait for worker readiness line
        let first_line = match stdout_lines.next() {
            Some(Ok(line)) => line,
            Some(Err(e)) => return Err(WorkerError::Died(format!("worker read failed: {e}"))),
            None => return Err(WorkerError::Died("worker exited before reporting readiness".to_string())),
        };

        let initial_reply: WorkerReply = serde_json::from_str(&first_line).map_err(|e| {
            WorkerError::Failed(format!("failed to parse worker handshake: {e}; raw: {first_line}"))
        })?;

        if initial_reply.ready != Some(true) {
            let err = initial_reply
                .error
                .unwrap_or_else(|| "worker reported ready=false".to_string());
            return Err(WorkerError::Failed(err));
        }

        let info = initial_reply.info.ok_or_else(|| {
            WorkerError::Failed("worker handshake succeeded but missing 'info' struct".to_string())
        })?;

        Ok(Self {
            info,
            child,
            stdin: Some(stdin),
            stdout_lines,
            next_id: 1,
            log_path: config.log,
        })
    }

    fn read_reply(&mut self) -> Result<WorkerReply, WorkerError> {
        let line = match self.stdout_lines.next() {
            Some(Ok(line)) => line,
            Some(Err(e)) => {
                return Err(WorkerError::Died(format!(
                    "error reading from worker: {e}{}",
                    self.log_tail()
                )))
            }
            None => {
                return Err(WorkerError::Died(format!(
                    "worker exited unexpectedly{}",
                    self.log_tail()
                )))
            }
        };

        let reply: WorkerReply = serde_json::from_str(&line).map_err(|e| {
            WorkerError::Failed(format!(
                "invalid JSON reply from worker: {e}; raw line: {line}"
            ))
        })?;
        Ok(reply)
    }

    fn send(
        &mut self,
        op: &str,
        inputs: &[String],
        budget: Option<usize>,
    ) -> Result<Vec<EmbeddingResult>, WorkerError> {
        let id = self.next_id;
        self.next_id += 1;

        let req = WorkerRequest {
            id,
            op: op.to_string(),
            inputs: Some(inputs.to_vec()),
            budget,
        };

        let json = serde_json::to_string(&req)?;
        if let Some(ref mut stdin) = self.stdin {
            writeln!(stdin, "{json}")?;
            stdin.flush()?;
        } else {
            return Err(WorkerError::Died("worker stdin is closed".to_string()));
        }

        let reply = self.read_reply()?;

        if reply.id != Some(id) {
            return Err(WorkerError::Failed(format!(
                "worker reply out of order: expected id {id}, got {:?}",
                reply.id
            )));
        }

        if reply.ok != Some(true) {
            return Err(WorkerError::Failed(
                reply
                    .error
                    .unwrap_or_else(|| "worker returned error without message".to_string()),
            ));
        }

        let encoded_vectors = reply
            .vectors
            .ok_or_else(|| WorkerError::Failed("reply marked ok but missing vectors".to_string()))?;

        let dim = reply.dim.unwrap_or(self.info.dim);
        let vectors = decode_f32_vectors(&encoded_vectors, inputs.len(), dim)?;

        let mut errors = reply.errors.unwrap_or_default();
        if errors.len() < inputs.len() {
            errors.resize(inputs.len(), None);
        }

        let mut results = Vec::with_capacity(inputs.len());
        for (vec, err) in vectors.into_iter().zip(errors.into_iter()) {
            if err.is_some() {
                results.push(EmbeddingResult {
                    vector: None,
                    error: err,
                });
            } else {
                results.push(EmbeddingResult {
                    vector: Some(vec),
                    error: None,
                });
            }
        }

        Ok(results)
    }

    fn log_tail(&self) -> String {
        if let Some(ref log_path) = self.log_path {
            if let Ok(content) = std::fs::read_to_string(log_path) {
                let lines: Vec<&str> = content.lines().collect();
                let tail_lines = lines.iter().rev().take(6).cloned().collect::<Vec<_>>();
                if !tail_lines.is_empty() {
                    let mut tail = tail_lines;
                    tail.reverse();
                    return format!(
                        "; worker log ({}):\n  {}",
                        log_path.display(),
                        tail.join("\n  ")
                    );
                }
            }
        }
        String::new()
    }
}

impl Embedder for EmbeddingWorker {
    fn info(&self) -> &WorkerInfo {
        &self.info
    }

    fn embed_texts(
        &mut self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let results = self.send("text", texts, None)?;
        let mut vectors = Vec::with_capacity(results.len());
        for res in results {
            if let Some(err) = res.error {
                return Err(format!("worker failed embedding text: {err}").into());
            }
            vectors.push(res.vector.unwrap_or_default());
        }
        Ok(vectors)
    }

    fn embed_images(
        &mut self,
        paths: &[String],
        budget: usize,
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let results = self.send("image", paths, Some(budget))?;
        Ok(results)
    }

    fn embed_audio(
        &mut self,
        paths: &[String],
    ) -> Result<Vec<EmbeddingResult>, Box<dyn std::error::Error + Send + Sync>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let results = self.send("audio", paths, None)?;
        Ok(results)
    }

    fn close(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let quit_req = WorkerRequest {
                id: 0,
                op: "quit".to_string(),
                inputs: None,
                budget: None,
            };
            if let Ok(json) = serde_json::to_string(&quit_req) {
                let _ = writeln!(stdin, "{json}");
                let _ = stdin.flush();
            }
        }

        let start = Instant::now();
        let timeout = Duration::from_secs(3);
        let mut exited = false;

        while start.elapsed() < timeout {
            if let Ok(Some(_)) = self.child.try_wait() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        if !exited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Drop for EmbeddingWorker {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f32_base64_roundtrip() {
        let original: Vec<Vec<f32>> = vec![
            vec![1.0, -0.5, 3.14159, 0.0],
            vec![-2.71828, 42.0, 100.25, -99.99],
        ];

        let encoded = encode_f32_vectors(&original);
        let decoded = decode_f32_vectors(&encoded, 2, 4).expect("decode failed");

        assert_eq!(original.len(), decoded.len());
        for (orig_row, dec_row) in original.iter().zip(decoded.iter()) {
            assert_eq!(orig_row.len(), dec_row.len());
            for (&o, &d) in orig_row.iter().zip(dec_row.iter()) {
                assert!((o - d).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn test_worker_request_reply_serialization() {
        let req = WorkerRequest {
            id: 42,
            op: "text".to_string(),
            inputs: Some(vec!["query: hello world".to_string()]),
            budget: None,
        };

        let json = serde_json::to_string(&req).expect("serialize req");
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"op\":\"text\""));
        assert!(!json.contains("budget")); // skipped if none

        let reply_json = r#"{
            "id": 42,
            "ok": true,
            "dim": 768,
            "count": 1,
            "vectors": "",
            "errors": [null]
        }"#;

        let reply: WorkerReply = serde_json::from_str(reply_json).expect("deserialize reply");
        assert_eq!(reply.id, Some(42));
        assert_eq!(reply.ok, Some(true));
        assert_eq!(reply.dim, Some(768));
    }

    #[test]
    fn test_worker_info_vector_source() {
        let info = WorkerInfo {
            model: "google/embeddinggemma-2".to_string(),
            revision: "7db9537abcdef123".to_string(),
            runtime: "mlx-0.22.0".to_string(),
            dtype: "bf16".to_string(),
            dim: 768,
            text_only: false,
            load_seconds: Some(1.25),
        };

        assert_eq!(
            info.vector_source(),
            "google/embeddinggemma-2@7db9537 mlx-0.22.0 bf16"
        );
    }
}
