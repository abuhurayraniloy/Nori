use nori_core::options::IndexOptions;
use nori_core::types::FileKind;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Classification outcomes for discovered files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// File should be indexed under the given `FileKind`.
    Index(FileKind),
    /// File is explicitly skipped with a human-readable reason (e.g. "minified", "license").
    Skip(String),
    /// File type is unhandled/ignored by the ingestion pipeline.
    Ignore,
}

/// Static rule constants matching Nori's ingestion specification.
pub struct Rules;

pub static SKIPPED_FOLDER_NAMES: &[&str] = &[
    "node_modules",
    "bower_components",
    "venv",
    "site-packages",
    "__pycache__",
    "DerivedData",
    "Pods",
    "Carthage",
];

pub static PROJECT_MARKERS: &[&str] = &[
    ".git",
    "package.json",
    "Package.swift",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    "Cargo.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "CMakeLists.txt",
];

pub static PROJECT_BUNDLE_EXTENSIONS: &[&str] = &["xcodeproj", "xcworkspace"];

pub static DOC_EXTENSIONS: &[&str] = &[
    "txt", "text", "md", "markdown", "rtf", "docx", "doc", "odt", "html", "htm",
];

pub static CODE_EXTENSIONS: &[&str] = &[
    "py", "ipynb", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "m", "mm", "swift", "tsx", "cts",
    "js", "jsx", "mjs", "cjs", "go", "rs", "java", "kt", "kts", "scala", "rb", "php", "cs", "fs",
    "sh", "bash", "zsh", "fish", "ps1", "lua", "pl", "r", "jl", "dart", "sql", "vue", "svelte",
    "css", "scss", "sass", "less", "json", "jsonl", "yaml", "yml", "toml", "xml", "plist",
    "gradle", "cmake", "mk", "lock", "log", "csv", "tsv", "ini", "cfg", "conf", "proto",
    "graphql", "tf", "nix", "zig", "hs", "ex", "exs", "erl", "clj", "elm", "ml", "vim", "el",
    "asm", "metal", "glsl", "hlsl", "wgsl", "cu", "pyi", "pyc",
];

pub static SCREENSHOT_PREFIXES: &[&str] = &["Screenshot", "Screen Shot", "CleanShot", "SCR-"];

pub static SOURCE_EXTENSIONS: &[&str] = &[
    "py", "pyi", "pyx", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx", "m", "mm", "swift",
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "go", "rs", "java", "kt", "kts",
    "scala", "groovy", "rb", "php", "cs", "fs", "sh", "bash", "zsh", "fish", "ps1", "lua", "pl",
    "pm", "r", "jl", "dart", "sql", "vue", "svelte", "astro", "html", "htm", "css", "scss",
    "sass", "less", "gradle", "cmake", "mk", "proto", "graphql", "gql", "tf", "hcl", "nix", "zig",
    "hs", "ex", "exs", "erl", "clj", "cljs", "elm", "ml", "mli", "vim", "el", "lisp", "asm", "s",
    "metal", "glsl", "hlsl", "wgsl", "cu", "cuh", "ino", "v", "sv", "tex",
];

pub static CODE_TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "rst", "adoc", "org", "txt", "yaml", "yml", "toml",
];

pub static CODE_FILE_NAMES: &[&str] = &[
    "Makefile",
    "makefile",
    "GNUmakefile",
    "Dockerfile",
    "Containerfile",
    "Justfile",
    "justfile",
    "Rakefile",
    "Gemfile",
    "Podfile",
    "Brewfile",
    "Procfile",
    "Vagrantfile",
    "Jenkinsfile",
];

pub static CODE_SKIPPED_FOLDER_NAMES: &[&str] = &[
    "vendor",
    "Vendor",
    "vendored",
    "third_party",
    "third-party",
    "thirdparty",
    "3rdparty",
    "external",
    "extern",
    "deps",
    "build",
    "Build",
    "dist",
    "out",
    "target",
    "obj",
    "coverage",
    "htmlcov",
    "generated",
    "__generated__",
    "wandb",
    "mlruns",
    "lightning_logs",
];

pub static IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif",
    "avif", "psd", "tga", "pnm", "ppm", "pgm", "cr2", "nef", "arw", "dng", "raf", "orf", "rw2",
];

pub static AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "wav", "wave", "flac", "ogg", "oga", "opus", "aiff", "aif",
    "wma", "amr", "caf", "mka",
];

pub static VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "3gp", "3g2", "mpg", "mpeg", "m2v",
    "ogv", "mxf",
];

pub static SECRET_TYPES: &[&str] = &[
    "pem",
    "key",
    "p12",
    "pfx",
    "keystore",
    "jks",
    "ppk",
    "kdbx",
    "gpg",
    "asc",
    "crt",
    "cer",
    "der",
    "p8",
    "mobileprovision",
];

pub static SECRET_PREFIXES: &[&str] = &[
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    ".env",
    "secret",
    "credential",
    ".netrc",
    ".npmrc",
    ".pypirc",
    "htpasswd",
    ".htpasswd",
];

pub static CAMERA_PREFIXES: &[&str] = &[
    "img", "dsc", "pxl", "mvimg", "vid", "gopr", "dji", "photo", "whatsapp", "screenshot",
];

impl Rules {
    /// Classifies an arbitrary file path using global index options (standard desktop search).
    pub fn classify(path: &Path, options: &IndexOptions) -> Classification {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if options.excluded_extensions.contains(&ext) {
            return Classification::Skip("excluded type".to_string());
        }

        // .ts / .mts are usually TypeScript. Only a real transport stream is video.
        if ext == "ts" || ext == "mts" || ext == "m2ts" {
            if Self::is_mpeg_transport_stream(path) {
                return Classification::Index(FileKind::Video);
            }
            return if options.include_code {
                Classification::Index(FileKind::Doc)
            } else {
                Classification::Skip("code or data file".to_string())
            };
        }

        if DOC_EXTENSIONS.contains(&ext.as_str()) {
            return Classification::Index(FileKind::Doc);
        }

        if CODE_EXTENSIONS.contains(&ext.as_str()) || SOURCE_EXTENSIONS.contains(&ext.as_str()) {
            return if options.include_code {
                Classification::Index(FileKind::Doc)
            } else {
                Classification::Skip("code or data file".to_string())
            };
        }

        if ext == "pdf" {
            return Classification::Index(FileKind::Pdf);
        }

        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            let kind = if Self::is_screenshot(path) {
                FileKind::Screenshot
            } else {
                FileKind::Image
            };
            return Classification::Index(kind);
        }

        if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            return Classification::Index(FileKind::Video);
        }

        if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            return Classification::Index(FileKind::Audio);
        }

        Classification::Ignore
    }

    /// Classifies a file for code-oriented search (`IndexOptions.code`).
    pub fn classify_code(path: &Path, options: &IndexOptions) -> Classification {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if options.excluded_extensions.contains(&ext) {
            return Classification::Skip("excluded type".to_string());
        }

        if Self::is_secret(name) {
            return Classification::Skip("key or password file".to_string());
        }

        let lower = name.to_lowercase();
        if lower.ends_with(".min.js") || lower.ends_with(".min.mjs") || lower.ends_with(".min.css") {
            return Classification::Skip("minified".to_string());
        }

        if ["license", "licence", "copying", "notice"]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            return Classification::Skip("license".to_string());
        }

        if (ext == "ts" || ext == "mts") && Self::is_mpeg_transport_stream(path) {
            return Classification::Ignore; // It's a video
        }

        if SOURCE_EXTENSIONS.contains(&ext.as_str())
            || CODE_TEXT_EXTENSIONS.contains(&ext.as_str())
            || ext == "ipynb"
            || CODE_FILE_NAMES.contains(&name)
            || name.starts_with("Dockerfile")
        {
            return Classification::Index(FileKind::Code);
        }

        if CODE_EXTENSIONS.contains(&ext.as_str()) {
            return Classification::Skip("data file".to_string());
        }

        Classification::Ignore
    }

    /// Checks whether a file name or extension represents private credentials/keys.
    pub fn is_secret(name: &str) -> bool {
        let lower = name.to_lowercase();
        if let Some(pos) = lower.rfind('.') {
            let ext = &lower[pos + 1..];
            if SECRET_TYPES.contains(&ext) {
                return true;
            }
        }
        SECRET_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
    }

    /// MPEG-TS packets start with 0x47 every 188 bytes (or 192 bytes for 4-byte timecode prefix).
    pub fn is_mpeg_transport_stream(path: &Path) -> bool {
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let mut buffer = [0u8; 600];
        let count = match file.read(&mut buffer) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if count < 400 {
            return false;
        }
        (buffer[0] == 0x47 && buffer[188] == 0x47 && buffer[376] == 0x47)
            || (buffer[4] == 0x47 && buffer[196] == 0x47 && buffer[388] == 0x47)
    }

    /// Tests if a file is a screenshot based on naming prefixes (and macOS xattr where available).
    pub fn is_screenshot(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        SCREENSHOT_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    }

    /// Determines why code search skips a folder.
    pub fn code_skip_reason(folder: &Path) -> Option<&'static str> {
        if let Some(name) = folder.file_name().and_then(|n| n.to_str()) {
            if CODE_SKIPPED_FOLDER_NAMES.contains(&name) {
                return Some("vendored or built code");
            }
        }
        let dot_git = folder.join(".git");
        if dot_git.is_file() {
            return Some("submodule or worktree");
        }
        None
    }

    /// Determines why general crawler skips a folder.
    pub fn skip_reason(folder: &Path, home: &Path, excluded: &[PathBuf]) -> Option<&'static str> {
        let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if SKIPPED_FOLDER_NAMES.contains(&name) {
            return Some("developer folder");
        }
        if name.ends_with(".noindex") {
            return Some(".noindex folder");
        }
        if folder == home.join("Library") {
            return Some("~/Library");
        }
        if excluded.iter().any(|ex| folder == ex || folder.starts_with(ex)) {
            return Some("excluded folder");
        }
        None
    }

    /// Checks if a path is inside a traversed walk, avoiding skipped/hidden subdirectories.
    pub fn is_walked(path: &Path, root: &Path, home: &Path, excluded: &[PathBuf]) -> bool {
        if path == root {
            return true;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            return false;
        };

        let mut current = root.to_path_buf();
        for comp in rel.components() {
            let part = comp.as_os_str().to_string_lossy();
            if part.starts_with('.') {
                return false;
            }
            current.push(comp);
            if Self::skip_reason(&current, home, excluded).is_some() {
                return false;
            }
        }
        true
    }

    /// Checks if a directory contains project markers (.git, Cargo.toml, package.json, etc.).
    pub fn is_project_root(folder: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return false;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if PROJECT_MARKERS.contains(&name_str.as_ref()) {
                return true;
            }
            if let Some(ext) = Path::new(&*name_str).extension().and_then(|e| e.to_str()) {
                if PROJECT_BUNDLE_EXTENSIONS.contains(&ext) {
                    return true;
                }
            }
        }
        false
    }

    /// Detects synthetic machine-generated names (e.g., `clip_000123`, `frame-0042`, UUIDs, MD5s).
    pub fn looks_machine_named(stem: &str) -> bool {
        let name = stem.to_lowercase();
        if CAMERA_PREFIXES.iter().any(|p| name.starts_with(p)) {
            return false;
        }

        static HEX_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"^[0-9a-f]{8,}([-_][0-9a-f]{4,})*$").unwrap()
        });

        static NUMBERED_PATTERN: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| {
                regex::Regex::new(r"^[a-z]{0,16}[-_. ]?\d{3,}([-_.]\d+)*$").unwrap()
            });

        if HEX_PATTERN.is_match(&name) {
            return true;
        }
        NUMBERED_PATTERN.is_match(&name)
    }

    /// Identifies ML training dataset folders (>= 300 items with >= 70% machine-named stems).
    pub fn dataset_folders<P: AsRef<Path>>(
        files: &[(P, &str, FileKind)],
        roots: &HashSet<PathBuf>,
        minimum: usize,
    ) -> HashMap<PathBuf, usize> {
        let mut groups: HashMap<(PathBuf, FileKind), Vec<String>> = HashMap::new();

        for (folder, stem, kind) in files {
            let folder_path = folder.as_ref().to_path_buf();
            if matches!(kind, FileKind::Image | FileKind::Audio | FileKind::Video)
                && !roots.contains(&folder_path)
            {
                groups
                    .entry((folder_path, *kind))
                    .or_default()
                    .push((*stem).to_string());
            }
        }

        let mut datasets = HashMap::new();
        for ((folder, _kind), stems) in groups {
            if stems.len() >= minimum {
                let machine_count = stems.iter().filter(|s| Self::looks_machine_named(s)).count();
                if (machine_count as f64) / (stems.len() as f64) >= 0.7 {
                    *datasets.entry(folder).or_insert(0) += stems.len();
                }
            }
        }
        datasets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_secret() {
        assert!(Rules::is_secret(".env"));
        assert!(Rules::is_secret(".env.local"));
        assert!(Rules::is_secret(".env.production"));
        assert!(Rules::is_secret("id_rsa"));
        assert!(Rules::is_secret("id_rsa.pub"));
        assert!(Rules::is_secret("id_ed25519"));
        assert!(Rules::is_secret("credentials.json"));
        assert!(Rules::is_secret("service_account.key"));
        assert!(Rules::is_secret("client.pem"));
        assert!(Rules::is_secret("keystore.jks"));
        assert!(Rules::is_secret("auth.pfx"));
        assert!(Rules::is_secret(".htpasswd"));

        assert!(!Rules::is_secret("main.rs"));
        assert!(!Rules::is_secret("notes.txt"));
        assert!(!Rules::is_secret("receipt.pdf"));
        assert!(!Rules::is_secret("photo.png"));
    }

    #[test]
    fn test_looks_machine_named() {
        // Camera / human photos
        assert!(!Rules::looks_machine_named("IMG_1234"));
        assert!(!Rules::looks_machine_named("PXL_20260101_123456789"));
        assert!(!Rules::looks_machine_named("Screenshot 2026-10-01 at 9.41.07 PM"));
        assert!(!Rules::looks_machine_named("Tokyo itinerary"));

        // Machine dataset files
        assert!(Rules::looks_machine_named("clip_000123"));
        assert!(Rules::looks_machine_named("frame-0042"));
        assert!(Rules::looks_machine_named("3f9a1c0e-12ab-4cde"));
        assert!(Rules::looks_machine_named("000123"));
    }

    #[test]
    fn test_dataset_quarantine() {
        let roots: HashSet<PathBuf> = [PathBuf::from("/Users/me/Photos")].into_iter().collect();
        let dataset_folder = PathBuf::from("/Users/me/Downloads/coco_train");
        let photos_folder = PathBuf::from("/Users/me/Photos/Vacation");

        let mut files = Vec::new();
        // 350 machine-named images in coco_train
        for _ in 0..350 {
            files.push((
                dataset_folder.as_path(),
                "frame_000123",
                FileKind::Image,
            ));
        }
        // 350 camera photos in Vacation
        for _ in 0..350 {
            files.push((
                photos_folder.as_path(),
                "IMG_4567",
                FileKind::Image,
            ));
        }

        let quarantined = Rules::dataset_folders(&files, &roots, 300);
        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined.get(&dataset_folder), Some(&350));
        assert!(!quarantined.contains_key(&photos_folder));
    }

    #[test]
    fn test_classify_general() {
        let default_options = IndexOptions::default();

        assert_eq!(
            Rules::classify(Path::new("photo.jpg"), &default_options),
            Classification::Index(FileKind::Image)
        );
        assert_eq!(
            Rules::classify(Path::new("Screenshot 2026.png"), &default_options),
            Classification::Index(FileKind::Screenshot)
        );
        assert_eq!(
            Rules::classify(Path::new("CleanShot 2026.png"), &default_options),
            Classification::Index(FileKind::Screenshot)
        );
        assert_eq!(
            Rules::classify(Path::new("document.pdf"), &default_options),
            Classification::Index(FileKind::Pdf)
        );
        assert_eq!(
            Rules::classify(Path::new("audio.mp3"), &default_options),
            Classification::Index(FileKind::Audio)
        );
        assert_eq!(
            Rules::classify(Path::new("video.mp4"), &default_options),
            Classification::Index(FileKind::Video)
        );
        assert_eq!(
            Rules::classify(Path::new("notes.md"), &default_options),
            Classification::Index(FileKind::Doc)
        );
        // Source code is skipped by default
        assert_eq!(
            Rules::classify(Path::new("main.rs"), &default_options),
            Classification::Skip("code or data file".to_string())
        );

        // With include_code = true
        let mut code_options = IndexOptions::default();
        code_options.include_code = true;
        assert_eq!(
            Rules::classify(Path::new("main.rs"), &code_options),
            Classification::Index(FileKind::Doc)
        );
    }

    #[test]
    fn test_classify_code() {
        let options = IndexOptions::default();

        assert_eq!(
            Rules::classify_code(Path::new("src/main.rs"), &options),
            Classification::Index(FileKind::Code)
        );
        assert_eq!(
            Rules::classify_code(Path::new("notebook.ipynb"), &options),
            Classification::Index(FileKind::Code)
        );
        assert_eq!(
            Rules::classify_code(Path::new("bundle.min.js"), &options),
            Classification::Skip("minified".to_string())
        );
        assert_eq!(
            Rules::classify_code(Path::new("LICENSE"), &options),
            Classification::Skip("license".to_string())
        );
        assert_eq!(
            Rules::classify_code(Path::new(".env"), &options),
            Classification::Skip("key or password file".to_string())
        );
        assert_eq!(
            Rules::classify_code(Path::new("dataset.csv"), &options),
            Classification::Skip("data file".to_string())
        );
    }

    #[test]
    fn test_is_walked() {
        let root = Path::new("/Users/me/Projects");
        let home = Path::new("/Users/me");
        let excluded = vec![PathBuf::from("/Users/me/Projects/secret_folder")];

        assert!(Rules::is_walked(
            Path::new("/Users/me/Projects/app/src"),
            root,
            home,
            &excluded
        ));
        // Hidden directory skipped
        assert!(!Rules::is_walked(
            Path::new("/Users/me/Projects/.git/objects"),
            root,
            home,
            &excluded
        ));
        // Developer directory skipped
        assert!(!Rules::is_walked(
            Path::new("/Users/me/Projects/app/node_modules/lodash"),
            root,
            home,
            &excluded
        ));
        // Excluded folder skipped
        assert!(!Rules::is_walked(
            Path::new("/Users/me/Projects/secret_folder/data"),
            root,
            home,
            &excluded
        ));
    }
}
