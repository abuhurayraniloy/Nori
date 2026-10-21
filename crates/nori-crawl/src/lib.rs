pub mod candidate;
pub mod crawler;
pub mod estimator;
pub mod git;

pub use candidate::{Candidate, CrawlReport};
pub use crawler::Crawler;
pub use estimator::{CodeIndexSize, Costs, Estimate, Estimator};
pub use git::{GitFiles, GitRepos};

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::options::IndexOptions;
    use nori_core::types::FileKind;
    use std::fs::{create_dir_all, File};
    use std::io::Write;

    #[test]
    fn test_crawler_respects_skip_folders_and_classifies_files() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("nori_crawl_test_{nanos}"));
        create_dir_all(&temp_dir).unwrap();

        // 1. Regular doc file
        let doc_path = temp_dir.join("guide.md");
        File::create(&doc_path)
            .unwrap()
            .write_all(b"# Quick Start Guide\nWelcome to Nori.")
            .unwrap();

        // 2. Secret file (should be skipped)
        let secret_path = temp_dir.join(".env");
        File::create(&secret_path)
            .unwrap()
            .write_all(b"API_KEY=12345")
            .unwrap();

        // 3. Ignored directory (.git or node_modules)
        let node_modules = temp_dir.join("node_modules");
        create_dir_all(&node_modules).unwrap();
        let js_path = node_modules.join("dep.js");
        File::create(&js_path)
            .unwrap()
            .write_all(b"console.log('vendor');")
            .unwrap();

        // 4. Code file in root
        let code_path = temp_dir.join("main.rs");
        File::create(&code_path)
            .unwrap()
            .write_all(b"fn main() {}")
            .unwrap();

        let mut options = IndexOptions::default();
        options.code = true;

        let report = Crawler::crawl(&[temp_dir.clone()], &options);

        // Verify code is found
        assert!(report.candidates.iter().any(|c| c.kind == FileKind::Code && c.path.ends_with("main.rs")));
        // Verify node_modules folder was skipped (reason: "developer folder")
        assert!(report.skipped_folders.contains_key("developer folder"));
        // Verify .env was skipped (reason: "key or password file")
        assert!(report.skipped.contains_key("key or password file"));
        // Verify no candidate inside node_modules was indexed
        assert!(!report.candidates.iter().any(|c| c.path.contains("node_modules")));

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
