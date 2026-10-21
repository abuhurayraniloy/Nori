use crate::candidate::{Candidate, CrawlReport};
use crate::git::GitRepos;
use nori_core::options::IndexOptions;
use nori_core::types::FileKind;
use nori_extract::folders::FolderSelection;
use nori_extract::rules::{Classification, Rules};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Crawler;

impl Crawler {
    /// Recursively discovers and filters candidate files across specified root directories.
    pub fn crawl(roots: &[PathBuf], options: &IndexOptions) -> CrawlReport {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut report = CrawlReport::default();
        let mut project_roots = HashSet::new();
        let mut repos = GitRepos::new();
        let mut found = Vec::new();
        let mut found_paths = HashSet::new();

        let root_set: HashSet<PathBuf> = roots.iter().cloned().collect();

        for root in roots {
            if options.code {
                repos.enter_root(root);
            } else if Rules::is_project_root(root) {
                project_roots.insert(root.clone());
            }

            let root_str = root.to_string_lossy();
            let real_root = FolderSelection::real_path(&root_str);
            let skips: Vec<PathBuf> = options
                .excluded_folders
                .iter()
                .filter(|ex| {
                    FolderSelection::is_inside(ex, &root_str)
                        || FolderSelection::is_inside(ex, &real_root)
                })
                .flat_map(|ex| vec![PathBuf::from(ex), PathBuf::from(FolderSelection::real_path(ex))])
                .collect();

            let mut it = WalkDir::new(root).into_iter();

            while let Some(entry_res) = it.next() {
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let path = entry.path();
                let file_type = entry.file_type();

                if file_type.is_dir() {
                    if path == root {
                        continue;
                    }

                    let reason = Rules::skip_reason(path, &home, &skips).or_else(|| {
                        if options.code {
                            Rules::code_skip_reason(path)
                        } else {
                            None
                        }
                    });

                    if let Some(r) = reason {
                        it.skip_current_dir();
                        *report.skipped_folders.entry(r.to_string()).or_insert(0) += 1;
                    } else if options.code {
                        repos.enter(path);
                    } else if Rules::is_project_root(path) {
                        project_roots.insert(path.to_path_buf());
                    }
                    continue;
                }

                if !file_type.is_file() {
                    *report
                        .skipped
                        .entry("link or special file".to_string())
                        .or_insert(0) += 1;
                    continue;
                }

                let classification = if options.code {
                    Rules::classify_code(path, options)
                } else {
                    Rules::classify(path, options)
                };

                match classification {
                    Classification::Ignore => {
                        *report.skipped.entry("other type".to_string()).or_insert(0) += 1;
                    }
                    Classification::Skip(reason) => {
                        *report.skipped.entry(reason.to_string()).or_insert(0) += 1;
                    }
                    Classification::Index(kind) => {
                        let meta = match std::fs::metadata(path) {
                            Ok(m) => m,
                            Err(_) => {
                                *report
                                    .skipped
                                    .entry("unreadable metadata".to_string())
                                    .or_insert(0) += 1;
                                continue;
                            }
                        };

                        let size = meta.len() as i64;
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs_f64())
                            .unwrap_or(0.0);

                        let (inode, device) = get_file_identifiers(path, &meta);

                        if options.code {
                            let is_ipynb = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|s| s.eq_ignore_ascii_case("ipynb"))
                                .unwrap_or(false);

                            let limit = if is_ipynb {
                                options.max_notebook_bytes
                            } else {
                                options.max_code_bytes
                            };

                            if size > limit as i64 {
                                *report
                                    .skipped
                                    .entry("too big to be written by hand".to_string())
                                    .or_insert(0) += 1;
                                continue;
                            }

                            if repos.ignores(path) {
                                *report
                                    .skipped
                                    .entry("ignored by git".to_string())
                                    .or_insert(0) += 1;
                                continue;
                            }
                        }

                        let path_str = path.to_string_lossy().to_string();
                        if found_paths.insert(path_str.clone()) {
                            found.push(Candidate {
                                path: path_str,
                                kind,
                                size,
                                mtime,
                                inode,
                                device,
                            });
                        }
                    }
                }
            }
        }

        // Post-crawl Step 1: Filter out files inside general code project roots unless they are screenshots
        let mut in_project_cache: HashMap<PathBuf, bool> = HashMap::new();
        let mut is_in_project = |folder: &Path| -> bool {
            let mut curr = folder.to_path_buf();
            while curr.parent().is_some() {
                if let Some(&ans) = in_project_cache.get(&curr) {
                    return ans;
                }
                if project_roots.contains(&curr) {
                    in_project_cache.insert(folder.to_path_buf(), true);
                    return true;
                }
                if !curr.pop() {
                    break;
                }
            }
            in_project_cache.insert(folder.to_path_buf(), false);
            false
        };

        let mut kept = Vec::new();
        for file in found {
            let p = Path::new(&file.path);
            let parent = p.parent().unwrap_or(p);

            if !options.code && file.kind != FileKind::Screenshot && is_in_project(parent) {
                *report
                    .skipped
                    .entry("inside a code project".to_string())
                    .or_insert(0) += 1;
            } else {
                kept.push(file);
            }
        }

        // Post-crawl Step 2: Quarantine dataset folders (>= 300 items with >= 70% machine-named stems)
        if !options.include_datasets && !options.code {
            let file_triplets: Vec<(PathBuf, &str, FileKind)> = kept
                .iter()
                .map(|c| {
                    let p = Path::new(&c.path);
                    let parent = p.parent().unwrap_or(p).to_path_buf();
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    (parent, stem, c.kind)
                })
                .collect();

            let datasets = Rules::dataset_folders(&file_triplets, &root_set, 300);
            if !datasets.is_empty() {
                for (p, count) in &datasets {
                    report.datasets.insert(p.to_string_lossy().to_string(), *count);
                }
                kept.retain(|file| {
                    let p = Path::new(&file.path);
                    let parent = p.parent().unwrap_or(p).to_path_buf();
                    let is_media = matches!(
                        file.kind,
                        FileKind::Image | FileKind::Audio | FileKind::Video
                    );
                    let is_dataset = datasets.contains_key(&parent) && is_media;
                    if is_dataset {
                        *report.skipped.entry("dataset".to_string()).or_insert(0) += 1;
                    }
                    !is_dataset
                });
            }
        }

        report.candidates = kept;
        report
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

/// Cross-platform inode and device extraction without unstable Rust APIs.
#[allow(unused_variables)]
fn get_file_identifiers(path: &Path, meta: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            #[repr(C)]
            struct ByHandleFileInformation {
                dw_file_attributes: u32,
                ft_creation_time: [u32; 2],
                ft_last_access_time: [u32; 2],
                ft_last_write_time: [u32; 2],
                dw_volume_serial_number: u32,
                n_file_size_high: u32,
                n_file_size_low: u32,
                n_number_of_links: u32,
                n_file_index_high: u32,
                n_file_index_low: u32,
            }

            unsafe extern "system" {
                fn CreateFileW(
                    lp_file_name: *const u16,
                    dw_desired_access: u32,
                    dw_share_mode: u32,
                    lp_security_attributes: *mut std::ffi::c_void,
                    dw_creation_disposition: u32,
                    dw_flags_and_attributes: u32,
                    h_template_file: *mut std::ffi::c_void,
                ) -> *mut std::ffi::c_void;

                fn GetFileInformationByHandle(
                    h_file: *mut std::ffi::c_void,
                    lp_file_information: *mut ByHandleFileInformation,
                ) -> i32;

                fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
            }

            const FILE_SHARE_READ: u32 = 1;
            const FILE_SHARE_WRITE: u32 = 2;
            const FILE_SHARE_DELETE: u32 = 4;
            const OPEN_EXISTING: u32 = 3;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
            const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

            let handle = CreateFileW(
                wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            );

            if handle != INVALID_HANDLE_VALUE {
                let mut info = std::mem::zeroed::<ByHandleFileInformation>();
                if GetFileInformationByHandle(handle, &mut info) != 0 {
                    CloseHandle(handle);
                    let inode = ((info.n_file_index_high as u64) << 32) | (info.n_file_index_low as u64);
                    let device = info.dw_volume_serial_number as u64;
                    return (inode, device);
                }
                CloseHandle(handle);
            }
        }
        (0, 0)
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.ino(), meta.dev())
    }

    #[cfg(not(any(windows, unix)))]
    {
        (0, 0)
    }
}
