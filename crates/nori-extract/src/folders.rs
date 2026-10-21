use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A folder's checkbox state in UI listings: all of it, some subfolders, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mark {
    On,
    Mixed,
    Off,
}

/// Which folders Nori searches: the folders chosen by the user, minus folders skipped inside them.
///
/// A skip applies inside the chosen folders that contain it, and a chosen folder is searched whole
/// even inside a skipped one (e.g. skip `Downloads/Media`, but still choose `Downloads/Media/Courses`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FolderSelection {
    pub chosen: Vec<String>,
    pub skipped: Vec<String>,
    /// Folders already searched: whatever is clicked, these remain kept.
    pub kept: Vec<String>,
}

impl FolderSelection {
    pub fn new(chosen: Vec<String>, skipped: Vec<String>, kept: Vec<String>) -> Self {
        Self {
            chosen: Self::unique(&chosen),
            skipped: Self::unique(&skipped),
            kept: Self::unique(&kept),
        }
    }

    /// Whether `path` (a folder or file) should be crawled and indexed.
    pub fn is_searched(&self, path: &str) -> bool {
        let mut all_folders = Vec::with_capacity(self.kept.len() + self.chosen.len());
        all_folders.extend_from_slice(&self.kept);
        all_folders.extend_from_slice(&self.chosen);
        Self::is_searched_with_lists(path, &all_folders, &self.skipped)
    }

    /// Checks if `path` is searched given specific folders and skipped rules.
    pub fn is_searched_with_lists(path: &str, folders: &[String], skipped: &[String]) -> bool {
        folders.iter().any(|folder| {
            (path == folder || Self::is_inside(path, folder))
                && !skipped.iter().any(|skip| {
                    Self::is_inside(skip, folder) && (path == skip || Self::is_inside(path, skip))
                })
        })
    }

    /// The closest chosen (or kept) folder that covers `path`.
    pub fn searched_as(&self, path: &str) -> Option<String> {
        let mut all_folders = Vec::with_capacity(self.kept.len() + self.chosen.len());
        all_folders.extend_from_slice(&self.kept);
        all_folders.extend_from_slice(&self.chosen);

        all_folders
            .into_iter()
            .filter(|folder| {
                (path == folder || Self::is_inside(path, folder))
                    && Self::is_searched_with_lists(path, std::slice::from_ref(folder), &self.skipped)
            })
            .max_by_key(|f| f.len())
    }

    /// The consolidated root folders to feed the crawler: kept and chosen folders,
    /// without those that another root already searches whole.
    pub fn folders(&self) -> Vec<String> {
        let mut all = Vec::with_capacity(self.kept.len() + self.chosen.len());
        all.extend_from_slice(&self.kept);
        all.extend_from_slice(&self.chosen);
        let all = Self::unique(&all);

        all.iter()
            .filter(|folder| {
                !all.iter().any(|other| {
                    other != *folder
                        && Self::is_inside(folder, other)
                        && Self::is_searched_with_lists(folder, std::slice::from_ref(other), &self.skipped)
                })
            })
            .cloned()
            .collect()
    }

    /// The skips that actively exclude content (i.e., sit inside a folder that is searched).
    pub fn effective_skips(&self) -> Vec<String> {
        let active_folders = self.folders();
        self.skipped
            .iter()
            .filter(|skip| active_folders.iter().any(|f| Self::is_inside(skip, f)))
            .cloned()
            .collect()
    }

    /// Returns the checkbox mark for a folder:
    /// - `.on`: whole folder is searched
    /// - `.mixed`: folder is searched with some subfolders skipped, or not searched but contains chosen subfolders
    /// - `.off`: not searched at all
    pub fn mark(&self, folder: &str) -> Mark {
        if self.is_searched(folder) {
            let has_skipped_child = self
                .skipped
                .iter()
                .any(|s| Self::parent(s) == folder && !self.is_searched(s));
            if has_skipped_child {
                Mark::Mixed
            } else {
                Mark::On
            }
        } else {
            let mut all = Vec::with_capacity(self.kept.len() + self.chosen.len());
            all.extend_from_slice(&self.kept);
            all.extend_from_slice(&self.chosen);
            let has_chosen_child = all.iter().any(|c| Self::parent(c) == folder);
            if has_chosen_child {
                Mark::Mixed
            } else {
                Mark::Off
            }
        }
    }

    /// Checks or unchecks a folder and all its direct subfolders.
    pub fn set_folder(&mut self, folder: &str, on: bool) {
        self.skipped.retain(|s| Self::parent(s) != folder);
        self.chosen.retain(|c| Self::parent(c) != folder);
        self.set_subfolder(folder, on);
    }

    /// Checks or unchecks a single folder, preserving existing subfolder choices inside it.
    pub fn set_subfolder(&mut self, folder: &str, on: bool) {
        if on {
            self.skipped.retain(|s| s != folder);
            if !self.is_searched(folder) {
                self.chosen.push(folder.to_string());
            }
        } else {
            self.chosen.retain(|c| c != folder);
            if self.is_searched(folder) {
                self.skipped.push(folder.to_string());
            }
        }
    }

    /// Removes a folder from the skipped list without adding it to chosen.
    pub fn unskip(&mut self, folder: &str) {
        self.skipped.retain(|s| s != folder);
    }

    /// Returns whether `path` is strictly inside `folder` (by whole path component boundaries).
    pub fn is_inside(path: &str, folder: &str) -> bool {
        let folder_trim = folder.trim_end_matches(['/', '\\']);
        if folder_trim.is_empty() {
            return path != folder;
        }
        if path.len() <= folder_trim.len() {
            return false;
        }
        if !path.starts_with(folder_trim) {
            return false;
        }
        let next_char = path.as_bytes()[folder_trim.len()];
        next_char == b'/' || next_char == b'\\'
    }

    /// Returns the immediate parent path string.
    pub fn parent(path: &str) -> String {
        let trimmed = path.trim_end_matches(['/', '\\']);
        if let Some(idx) = trimmed.rfind(['/', '\\']) {
            if idx == 0 {
                path[..1].to_string()
            } else if idx == 2 && trimmed.as_bytes()[1] == b':' {
                // Windows drive root: e.g. "C:\foo" -> "C:\"
                path[..=idx].to_string()
            } else {
                path[..idx].to_string()
            }
        } else {
            String::new()
        }
    }

    /// Resolves canonical path and symlinks without Windows extended `\\?\` prefix.
    pub fn real_path(path: &str) -> String {
        if let Ok(canon) = std::fs::canonicalize(path) {
            let s = canon.to_string_lossy();
            if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{}", stripped)
            } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
                stripped.to_string()
            } else {
                s.to_string()
            }
        } else {
            path.to_string()
        }
    }

    fn unique(paths: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for p in paths {
            if seen.insert(p.clone()) {
                result.push(p.clone());
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unchecking_a_subfolder_skips_it() {
        let mut selection = FolderSelection::new(vec!["/D".into()], vec![], vec![]);
        selection.set_subfolder("/D/Courses", false);
        assert_eq!(selection.skipped, vec!["/D/Courses"]);
        assert_eq!(selection.mark("/D"), Mark::Mixed);
        assert!(!selection.is_searched("/D/Courses/week1.mp4"));
        assert!(selection.is_searched("/D/Books/a.pdf"));

        // Clicking on a mixed checkbox turns all of it on
        selection.set_folder("/D", true);
        assert!(selection.skipped.is_empty());
        assert_eq!(selection.mark("/D"), Mark::On);
    }

    #[test]
    fn test_checking_a_subfolder_chooses_it_alone() {
        let mut selection = FolderSelection::default();
        selection.set_subfolder("/D/Receipts", true);
        assert_eq!(selection.chosen, vec!["/D/Receipts"]);
        assert_eq!(selection.mark("/D"), Mark::Mixed);
        assert!(selection.is_searched("/D/Receipts/may.pdf"));
        assert!(!selection.is_searched("/D/other.pdf"));
        assert_eq!(selection.folders(), vec!["/D/Receipts"]);

        selection.set_folder("/D", true);
        assert_eq!(selection.chosen, vec!["/D"]); // Covered now
        selection.set_folder("/D", false);
        assert!(selection.chosen.is_empty());
        assert_eq!(selection.mark("/D"), Mark::Off);
    }

    #[test]
    fn test_unchecking_a_folder_drops_subfolder_choices_but_not_deeper_skips() {
        let mut selection = FolderSelection::new(
            vec!["/D".into()],
            vec!["/D/A".into(), "/D/B/old".into()],
            vec![],
        );
        selection.set_folder("/D", false);
        assert!(selection.chosen.is_empty());
        assert_eq!(selection.skipped, vec!["/D/B/old"]);
        assert!(selection.effective_skips().is_empty()); // Nothing around it searched
    }

    #[test]
    fn test_chosen_folder_inside_skipped_one_is_searched_whole() {
        let mut selection =
            FolderSelection::new(vec!["/D".into()], vec!["/D/Media".into()], vec![]);
        selection.set_subfolder("/D/Media/Courses", true);
        assert_eq!(selection.chosen, vec!["/D", "/D/Media/Courses"]);
        assert!(selection.is_searched("/D/Media/Courses/week1/a.mp4"));
        assert!(!selection.is_searched("/D/Media/film.mp4"));
        assert_eq!(selection.folders(), vec!["/D", "/D/Media/Courses"]);

        selection.set_subfolder("/D/Media/Courses/old", false);
        assert!(!selection.is_searched("/D/Media/Courses/old/b.mp4"));
    }

    #[test]
    fn test_folders_inside_another_chosen_one_are_covered() {
        let selection =
            FolderSelection::new(vec!["/D/A".into(), "/D".into(), "/E".into()], vec![], vec![]);
        assert_eq!(selection.folders(), vec!["/D", "/E"]);
        assert_eq!(selection.searched_as("/D/A/x.png"), Some("/D/A".into()));
    }

    #[test]
    fn test_kept_folders_stay_whatever_is_clicked() {
        let mut selection = FolderSelection::new(vec![], vec![], vec!["/D/Receipts".into()]);
        assert_eq!(selection.mark("/D"), Mark::Mixed);
        selection.set_folder("/D", true);
        assert_eq!(selection.folders(), vec!["/D"]);
        selection.set_folder("/D", false);
        assert_eq!(selection.folders(), vec!["/D/Receipts"]);
        assert_eq!(selection.mark("/D"), Mark::Mixed);
    }

    #[test]
    fn test_checking_a_skipped_folder_takes_the_skip_back() {
        let mut selection =
            FolderSelection::new(vec![], vec!["/H/Movies".into()], vec!["/H".into()]);
        assert!(!selection.is_searched("/H/Movies"));
        selection.set_folder("/H/Movies", true);
        assert!(selection.skipped.is_empty());
        assert!(selection.chosen.is_empty()); // Searched as part of /H again
        selection.set_folder("/H/Movies", false);
        assert_eq!(selection.skipped, vec!["/H/Movies"]);
    }

    #[test]
    fn test_paths_are_compared_by_whole_names() {
        assert!(!FolderSelection::is_inside("/a/bc", "/a/b"));
        assert!(FolderSelection::is_inside("/a/b/c", "/a/b"));
        assert!(!FolderSelection::new(vec!["/a/b".into()], vec![], vec![]).is_searched("/a/bc/x"));
    }
}
