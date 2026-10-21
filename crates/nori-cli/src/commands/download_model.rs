//! Subcommand `nori download-model [--to DIR]`: downloads EmbeddingGemma 2 weights.
//! Streams JSON events on stdout matching the protocol expected by `nori-gui` and automated scripts.

use nori_core::media_files::{ModelFile, ModelFiles};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct DownloadModelArgs {
    pub to: Option<PathBuf>,
}

fn emit(val: serde_json::Value) {
    println!("{}", serde_json::to_string(&val).unwrap_or_default());
    let _ = std::io::stdout().flush();
}

pub fn run_download_model(args: DownloadModelArgs) -> Result<(), String> {
    let folder = args.to.unwrap_or_else(ModelFiles::models_dir);
    if let Err(e) = fs::create_dir_all(&folder) {
        let msg = format!("cannot create directory {}: {e}", folder.display());
        emit(json!({ "event": "error", "message": msg }));
        return Err(msg);
    }

    let missing = ModelFiles::missing_in(&folder);
    let missing_bytes: u64 = missing.iter().map(|f| f.bytes).sum();
    let mut done_bytes = ModelFiles::TOTAL_BYTES.saturating_sub(missing_bytes);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    for file in missing {
        let final_path = folder.join(file.name);
        let partial_path = folder.join(format!("{}.partial", file.name));

        // 1. Check Hugging Face hub local cache
        if let Some(cached) = ModelFiles::cached_copy(&file) {
            emit(json!({
                "event": "file",
                "name": file.name,
                "bytes": file.bytes,
                "from": "cache"
            }));
            emit(json!({ "event": "verifying", "name": file.name }));
            if let Ok(hash) = ModelFiles::verify_sha256(&cached) {
                if hash.eq_ignore_ascii_case(file.sha256) {
                    let _ = fs::remove_file(&partial_path);
                    if fs::copy(&cached, &partial_path).is_ok() && fs::rename(&partial_path, &final_path).is_ok() {
                        done_bytes += file.bytes;
                        emit(json!({
                            "event": "progress",
                            "name": file.name,
                            "bytes": file.bytes,
                            "total": file.bytes,
                            "all_bytes": done_bytes,
                            "all_total": ModelFiles::TOTAL_BYTES
                        }));
                        continue;
                    }
                }
            }
        }

        // 2. Download from Hugging Face
        emit(json!({
            "event": "file",
            "name": file.name,
            "bytes": file.bytes,
            "from": "download"
        }));

        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            ModelFiles::REPOSITORY,
            ModelFiles::REVISION,
            file.name
        );

        let mut failures = 0;
        loop {
            match download_one_file(&client, &file, &url, &partial_path, done_bytes) {
                Ok(()) => {
                    emit(json!({ "event": "verifying", "name": file.name }));
                    match ModelFiles::verify_sha256(&partial_path) {
                        Ok(hash) if hash.eq_ignore_ascii_case(file.sha256) => {
                            if let Err(e) = fs::rename(&partial_path, &final_path) {
                                let msg = format!("failed moving {} into place: {e}", file.name);
                                emit(json!({ "event": "error", "message": msg }));
                                return Err(msg);
                            }
                            done_bytes += file.bytes;
                            break;
                        }
                        Ok(hash) => {
                            let _ = fs::remove_file(&partial_path);
                            failures += 1;
                            if failures >= 3 {
                                let msg = format!("{} arrived damaged 3 times (hash mismatch: {})", file.name, hash);
                                emit(json!({ "event": "error", "message": msg }));
                                return Err(msg);
                            }
                            emit(json!({
                                "event": "retry",
                                "message": format!("{} arrived damaged, retrying", file.name),
                                "seconds": 0
                            }));
                        }
                        Err(e) => {
                            let _ = fs::remove_file(&partial_path);
                            failures += 1;
                            if failures >= 3 {
                                let msg = format!("failed verifying {}: {e}", file.name);
                                emit(json!({ "event": "error", "message": msg }));
                                return Err(msg);
                            }
                            emit(json!({
                                "event": "retry",
                                "message": format!("verification failed: {e}"),
                                "seconds": 1
                            }));
                            std::thread::sleep(Duration::from_secs(1));
                        }
                    }
                }
                Err(e) => {
                    failures += 1;
                    if failures > 15 {
                        let msg = format!("failed downloading {}: {e}", file.name);
                        emit(json!({ "event": "error", "message": msg }));
                        return Err(msg);
                    }
                    let delay = std::cmp::min(60, 1u64 << std::cmp::min(failures, 6));
                    emit(json!({
                        "event": "retry",
                        "message": format!("{e}"),
                        "seconds": delay
                    }));
                    std::thread::sleep(Duration::from_secs(delay));
                }
            }
        }
    }

    emit(json!({
        "event": "done",
        "folder": folder.to_string_lossy()
    }));
    Ok(())
}

fn download_one_file(
    client: &reqwest::blocking::Client,
    file: &ModelFile,
    url: &str,
    partial_path: &Path,
    before_bytes: u64,
) -> Result<(), String> {
    let mut downloaded = if let Ok(meta) = fs::metadata(partial_path) {
        meta.len()
    } else {
        0
    };

    if downloaded > file.bytes {
        let _ = fs::remove_file(partial_path);
        downloaded = 0;
    }

    let mut request = client.get(url);
    if downloaded > 0 {
        request = request.header("Range", format!("bytes={}-", downloaded));
    }

    let mut response = request.send().map_err(|e| format!("HTTP request failed: {e}"))?;
    let status = response.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("server returned status {}", status));
    }

    let mut file_out = OpenOptions::new()
        .create(true)
        .write(true)
        .append(downloaded > 0)
        .open(partial_path)
        .map_err(|e| format!("cannot open partial file: {e}"))?;

    let mut buffer = vec![0u8; 128 * 1024];
    let mut last_progress = Instant::now();

    loop {
        let n = response
            .read(&mut buffer)
            .map_err(|e| format!("stream read error: {e}"))?;
        if n == 0 {
            break;
        }
        file_out
            .write_all(&buffer[..n])
            .map_err(|e| format!("write error: {e}"))?;
        downloaded += n as u64;

        if last_progress.elapsed() >= Duration::from_millis(200) || downloaded >= file.bytes {
            last_progress = Instant::now();
            emit(json!({
                "event": "progress",
                "name": file.name,
                "bytes": downloaded,
                "total": file.bytes,
                "all_bytes": before_bytes + downloaded,
                "all_total": ModelFiles::TOTAL_BYTES
            }));
        }
    }

    file_out.flush().map_err(|e| format!("flush error: {e}"))?;
    Ok(())
}
