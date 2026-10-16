use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::{Path, PathBuf};

/// Cross platform standard application path for nori.
pub struct AppPaths;

impl AppPaths {
    /// Returns the application data directory.
    /// - Windows: `%LOCALAPPDATA%\Nori`
    /// - macOS: `~/Library/Application Support/Nori`
    /// - Linux: `$XDG_DATA_HOME/nori` or `~/.local/share/nori`

    pub fn application_support() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                if !local.trim().is_empty() {
                    return PathBuf::from(local).join("Nori");
                }
            }
            dirs_fallback().join("AppData").join("Local").join("Nori")
        }

        #[cfg(target_os = "macos")]
        {
            dirs_fallback()
                .join("Library")
                .join("Application Support")
                .join("Nori")
        }

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            if let Ok(data) = std::env::var("XDG_DATA_HOME") {
                if !data.trim().is_empty() {
                    return PathBuf::from(data).join("nori");
                }
            }
            dirs_fallback().join(".local").join("share").join("nori")
        }
    }

    /// The default index directory.
    pub fn default_index() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            Self::application_support().join("index.noindex")
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::application_support().join("index")
        }
    }
}

#[inline]
pub fn dirs_fallback() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

impl ModelFile {
    /// Hugging Face CDN address for this pinned revision.
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolver/{}/{}",
            ModelFiles::REPOSITORY,
            ModelFiles::REVISION,
            self.name
        )
    }

    /// Checks if a file exists on disk and matches the expected byte length.
    pub fn exists_with_valid_size(&self, path: &Path) -> bool {
        if let Ok(meta) = std::fs::metadata(path) {
            meta.len() == self.bytes
        } else {
            false
        }
    }
}

pub struct ModelFiles;

impl ModelFiles {
    pub const REPOSITORY: &'static str = "ggml-org/embeddinggemma-2-GGUF";
    pub const REVISION: &'static str = "bfcd298762cc34d0357ece5ebdd31791a3a374d8";

    /// The text model (310 MB): queries and text document embeddings.
    pub const TEXT: ModelFile = ModelFile {
        name: "embeddinggemma-2-Q8_0.gguf",
        bytes: 309_855_456,
        sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135",
    };
    /// The image and audio vision/audio projectors (555 MB).
    pub const MEDIA: ModelFile = ModelFile {
        name: "mmproj-embeddinggemma-2-Q8_0.gguf",
        bytes: 554_821_024,
        sha256: "c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f",
    };

    pub const ALL: [ModelFile; 2] = [Self::TEXT, Self::MEDIA];
    pub const TOTAL_BYTES: u64 = Self::TEXT.bytes + Self::MEDIA.bytes;

    pub fn models_dir() -> PathBuf {
        AppPaths::application_support().join("Models")
    }

    /// Looks up local snapshot directory from the Hugging Face Hub cache if present.
    pub fn huggingface_cache_snapshot() -> PathBuf {
        let repo_dirname = format!("models--{}", Self::REPOSITORY.replace('/', "--"));
        let hub = if let Ok(hub_cache) = std::env::var("HF_HUB_CACHE") {
            PathBuf::from(hub_cache)
        } else if let Ok(hf_home) = std::env::var("HF_HOME") {
            PathBuf::from(hf_home).join("hub")
        } else {
            dirs_fallback()
                .join(".cache")
                .join("huggingface")
                .join("hub")
        };
        hub.join(repo_dirname)
            .join("snapshots")
            .join(Self::REVISION)
    }

    /// Finds a copy in the Hugging Face cache matching size, if already present.
    pub fn cached_copy(file: &ModelFile) -> Option<PathBuf> {
        let candidate = Self::huggingface_cache_snapshot().join(file.name);
        if file.exists_with_valid_size(&candidate) {
            Some(candidate)
        } else {
            None
        }
    }

    /// Identifies model files that are missing from `folder`.
    pub fn missing_in(folder: &Path) -> Vec<ModelFile> {
        Self::ALL
            .iter()
            .filter(|f| !folder.join(f.name).exists())
            .cloned()
            .collect()
    }

    /// Computes the lowercase hex SHA-256 checksum of a file streaming in 8 MB chunks.
    pub fn verify_sha256(path: &Path) -> IoResult<String> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        // 8 MB chunk buffer keeps memory flat
        let mut buffer = vec![0u8; 8 * 1024 * 1024];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        let hash_bytes = hasher.finalize();
        Ok(hex::encode(hash_bytes))
    }
}
