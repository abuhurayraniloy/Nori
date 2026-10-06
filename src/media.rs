use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCategory {
    Documents,
    Images,
    Audio,
    Video,
    Archives,
    Installers,
    Code,
    Other,
}

impl MediaCategory {
    /// Categorizes a file based strictly on its file extension.
    pub fn from_path(path: &Path) -> Self {
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext.to_lowercase(),
            None => return MediaCategory::Other,
        };

        match ext.as_str() {
            // Documents
            "pdf" | "doc" | "docx" | "txt" | "rtf" | "odt" | "xls" | "xlsx" | "ppt" | "pptx"
            | "csv" | "epub" | "md" => MediaCategory::Documents,

            // Images
            "png" | "jpg" | "jpeg" | "webp" | "svg" | "gif" | "bmp" | "tiff" | "ico" | "heic" => {
                MediaCategory::Images
            }

            // Audio
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => MediaCategory::Audio,

            // Video
            "mp4" | "mkv" | "mov" | "avi" | "webm" | "flv" | "wmv" => MediaCategory::Video,

            // Archives
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => MediaCategory::Archives,

            // Installers & System images
            "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "iso" => MediaCategory::Installers,

            // Code & Data
            "rs" | "py" | "js" | "ts" | "html" | "css" | "c" | "cpp" | "h" | "hpp" | "java"
            | "go" | "json" | "yaml" | "yml" | "toml" | "xml" | "sql" | "sh" | "ps1" | "bat" => {
                MediaCategory::Code
            }

            // Fallback for everything else
            _ => MediaCategory::Other,
        }
    }

    /// Returns the standard folder name for this category.
    pub fn folder_name(&self) -> &'static str {
        match self {
            MediaCategory::Documents => "Documents",
            MediaCategory::Images => "Images",
            MediaCategory::Audio => "Audio",
            MediaCategory::Video => "Video",
            MediaCategory::Archives => "Archives",
            MediaCategory::Installers => "Installers",
            MediaCategory::Code => "Code",
            MediaCategory::Other => "Other",
        }
    }
}

/// Resolves a collision-safe destination path if a file already exists in target directory.
pub fn get_unique_destination(target_dir: &Path, file_name: &str) -> PathBuf {
    let mut dest = target_dir.join(file_name);
    if !dest.exists() {
        return dest;
    }

    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());

    let mut counter = 1;
    loop {
        let candidate_name = match ext {
            Some(ext) => format!("{}_{}.{}", stem, counter, ext),
            None => format!("{}_{}", stem, counter),
        };
        dest = target_dir.join(candidate_name);
        if !dest.exists() {
            return dest;
        }
        counter += 1;
    }
}

/// Moves a file into its extension-based media directory under `base_output_dir`.
pub fn sort_file_by_extension(file_path: &Path, base_output_dir: &Path) -> io::Result<PathBuf> {
    let category = MediaCategory::from_path(file_path);
    let target_dir = base_output_dir.join(category.folder_name());

    fs::create_dir_all(&target_dir)?;

    let file_name = match file_path.file_name().and_then(|f| f.to_str()) {
        Some(name) => name,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid file name",
            ));
        }
    };

    let destination = get_unique_destination(&target_dir, file_name);

    // Try an atomic rename first; if moving across mount points/drives, fall back to copy + delete.
    if fs::rename(file_path, &destination).is_err() {
        fs::copy(file_path, &destination)?;
        fs::remove_file(file_path)?;
    }

    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_media_category_mapping() {
        assert_eq!(
            MediaCategory::from_path(Path::new("paper.pdf")),
            MediaCategory::Documents
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("photo.JPG")),
            MediaCategory::Images
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("song.mp3")),
            MediaCategory::Audio
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("movie.mkv")),
            MediaCategory::Video
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("archive.tar.gz")),
            MediaCategory::Archives
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("setup.exe")),
            MediaCategory::Installers
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("main.rs")),
            MediaCategory::Code
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("data.bin")),
            MediaCategory::Other
        );
        assert_eq!(
            MediaCategory::from_path(Path::new("no_extension")),
            MediaCategory::Other
        );
    }

    #[test]
    fn test_unique_destination_collision() {
        let temp_dir = std::env::temp_dir().join("nori_test_collision");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let initial_file = temp_dir.join("test.txt");
        File::create(&initial_file).unwrap();

        let dest1 = get_unique_destination(&temp_dir, "test.txt");
        assert_eq!(dest1, temp_dir.join("test_1.txt"));

        File::create(&dest1).unwrap();
        let dest2 = get_unique_destination(&temp_dir, "test.txt");
        assert_eq!(dest2, temp_dir.join("test_2.txt"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sort_file_by_extension() {
        let temp_dir = std::env::temp_dir().join("nori_test_sort");
        let _ = fs::remove_dir_all(&temp_dir);
        let src_dir = temp_dir.join("src");
        let out_dir = temp_dir.join("out");
        fs::create_dir_all(&src_dir).unwrap();

        let test_file = src_dir.join("invoice.pdf");
        File::create(&test_file).unwrap();

        let dest = sort_file_by_extension(&test_file, &out_dir).unwrap();
        assert_eq!(dest, out_dir.join("Documents").join("invoice.pdf"));
        assert!(dest.exists());
        assert!(!test_file.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
