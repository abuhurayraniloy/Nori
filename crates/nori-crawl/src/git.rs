use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Queries `git ls-files` without modifying or locking the repository.
pub struct GitFiles;

impl GitFiles {
    /// Lists all tracked files and untracked non-ignored files relative to `repo`.
    /// Returns `None` if git is not installed, the folder is not a git repo, or execution fails.
    pub fn list<P: AsRef<Path>>(repo: P) -> Option<HashSet<String>> {
        let repo = repo.as_ref();

        #[cfg(windows)]
        let null_device = "NUL";
        #[cfg(not(windows))]
        let null_device = "/dev/null";

        let mut cmd = Command::new("git");
        cmd.args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            &format!("core.hooksPath={null_device}"),
            "-C",
            &repo.to_string_lossy(),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ]);

        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_CONFIG_GLOBAL", null_device);

        cmd.stdin(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.stdout(Stdio::piped());

        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }

        let mut set = HashSet::new();
        for slice in output.stdout.split(|&b| b == 0) {
            if !slice.is_empty() {
                if let Ok(s) = std::str::from_utf8(slice) {
                    set.insert(s.replace('\\', "/"));
                }
            }
        }

        Some(set)
    }
}

/// Tracks git repository roots and checks if individual files are ignored by git.
#[derive(Debug, Default)]
pub struct GitRepos {
    /// Maps repo root path -> set of kept files (relative to repo root).
    files: HashMap<PathBuf, Option<HashSet<String>>>,
    /// Maps directory path -> enclosing git repo root (if any).
    repo_of: HashMap<PathBuf, Option<PathBuf>>,
}

impl GitRepos {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a crawl root directory by walking upward looking for `.git`.
    pub fn enter_root<P: AsRef<Path>>(&mut self, path: P) {
        let mut curr: PathBuf = path.as_ref().to_path_buf();
        loop {
            if curr.join(".git").exists() {
                let kept = GitFiles::list(&curr);
                self.files.insert(curr.clone(), kept);
                self.repo_of.insert(path.as_ref().to_path_buf(), Some(curr));
                return;
            }
            if !curr.pop() {
                break;
            }
        }
        self.repo_of.insert(path.as_ref().to_path_buf(), None);
    }

    /// Registers a subfolder entered during traversal.
    pub fn enter<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref();
        let git_dir = p.join(".git");
        if git_dir.is_dir() {
            let kept = GitFiles::list(p);
            self.files.insert(p.to_path_buf(), kept);
            self.repo_of.insert(p.to_path_buf(), Some(p.to_path_buf()));
        }
    }

    /// Resolves which git repo contains `folder`.
    pub fn repo_of(&mut self, folder: &Path) -> Option<PathBuf> {
        if let Some(res) = self.repo_of.get(folder) {
            return res.clone();
        }

        let answer = if let Some(parent) = folder.parent() {
            if parent != folder && parent.as_os_str().len() > 1 {
                self.repo_of(parent)
            } else {
                None
            }
        } else {
            None
        };

        self.repo_of.insert(folder.to_path_buf(), answer.clone());
        answer
    }

    /// Returns `true` if `path` is inside a repository and git ignores it.
    pub fn ignores<P: AsRef<Path>>(&mut self, path: P) -> bool {
        let path = path.as_ref();
        let parent = match path.parent() {
            Some(p) => p,
            None => return false,
        };

        let repo_root = match self.repo_of(parent) {
            Some(r) => r,
            None => return false,
        };

        if let Some(Some(kept)) = self.files.get(&repo_root) {
            if let Ok(rel) = path.strip_prefix(&repo_root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                return !kept.contains(&rel_str);
            }
        }

        false
    }
}
