use crate::candidate::Candidate;
use nori_core::options::IndexOptions;
use nori_core::types::FileKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Empirical indexing costs in seconds per processing unit (measured on Apple Silicon / modern CPU).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Costs {
    pub image: f64,
    pub screenshot: f64,
    pub pdf_page: f64,
    pub doc_chunk: f64,
    pub audio_second: f64,
    pub video_second: f64,
    pub code_stretch: f64,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            image: 0.12,
            screenshot: 0.32,
            pdf_page: 0.09,
            doc_chunk: 0.045,
            audio_second: 0.0089,
            video_second: 0.019,
            code_stretch: 0.037,
        }
    }
}

pub struct CodeIndexSize;

impl CodeIndexSize {
    pub const PER_CHARACTER: f64 = 2.4;
    pub const PER_STRETCH: f64 = 1600.0;
    pub const CHARACTERS_PER_STRETCH: i64 = 1400;
}

/// Projected indexing workload, duration, and disk footprint.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub files: HashMap<FileKind, usize>,
    pub pdf_pages: usize,
    pub pdf_pages_total: usize,
    pub audio_seconds: f64,
    pub video_seconds: f64,
    pub doc_chunks: usize,
    pub seconds: f64,
    pub long_files: usize,
    pub later_seconds: f64,
    pub code_stretches: usize,
    pub bytes: f64,
    pub kind_seconds: HashMap<FileKind, f64>,
}

impl std::ops::Add for Estimate {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        for (k, v) in rhs.files {
            *self.files.entry(k).or_insert(0) += v;
        }
        self.pdf_pages += rhs.pdf_pages;
        self.pdf_pages_total += rhs.pdf_pages_total;
        self.audio_seconds += rhs.audio_seconds;
        self.video_seconds += rhs.video_seconds;
        self.doc_chunks += rhs.doc_chunks;
        self.seconds += rhs.seconds;
        self.long_files += rhs.long_files;
        self.later_seconds += rhs.later_seconds;
        self.code_stretches += rhs.code_stretches;
        self.bytes += rhs.bytes;
        for (k, v) in rhs.kind_seconds {
            *self.kind_seconds.entry(k).or_insert(0.0) += v;
        }
        self
    }
}

pub struct Estimator;

impl Estimator {
    /// Computes indexing duration and resource estimates from candidate file metadata without decoding contents.
    pub fn estimate(
        candidates: &[Candidate],
        options: &IndexOptions,
        custom_costs: Option<&Costs>,
    ) -> Estimate {
        let default_costs = Costs::default();
        let costs = custom_costs.unwrap_or(&default_costs);

        let mut estimate = Estimate::default();

        for file in candidates {
            *estimate.files.entry(file.kind).or_insert(0) += 1;
            let before = estimate.seconds;

            match file.kind {
                FileKind::Image => {
                    estimate.seconds += costs.image;
                }
                FileKind::Screenshot => {
                    estimate.seconds += costs.screenshot;
                }
                FileKind::Pdf => {
                    // Rough metadata estimate: ~50KB per page
                    let total = ((file.size / 50_000) as usize).max(1);
                    let all = total.min(options.max_pages);
                    let first = if total > options.big_pdf_pages {
                        options.pdf_step.min(all)
                    } else {
                        all
                    };

                    estimate.pdf_pages_total += total;
                    estimate.pdf_pages += first;
                    estimate.seconds += (first as f64) * costs.pdf_page;

                    if all > first {
                        estimate.long_files += 1;
                        estimate.later_seconds += ((all - first) as f64) * costs.pdf_page;
                    }
                }
                FileKind::Doc => {
                    let all = ((file.size / 2000) as usize)
                        .max(1)
                        .min(options.max_chunks);
                    let first = all.min(options.doc_step);

                    estimate.doc_chunks += first;
                    estimate.seconds += (first as f64) * costs.doc_chunk;

                    if all > first {
                        estimate.long_files += 1;
                        estimate.later_seconds += ((all - first) as f64) * costs.doc_chunk;
                    }
                }
                FileKind::Audio => {
                    // Rough estimate from file size (~128kbps = 16KB/s)
                    let duration = (file.size as f64 / 16_000.0).max(1.0);
                    estimate.audio_seconds += duration;
                    estimate.seconds += duration * costs.audio_second;
                }
                FileKind::Video => {
                    // Rough estimate from file size (~2MB/s = 2_000_000B/s)
                    let duration = (file.size as f64 / 2_000_000.0).max(1.0);
                    estimate.video_seconds += duration;
                    estimate.seconds += duration * costs.video_second;
                }
                FileKind::Code => {
                    let characters = if file.path.to_lowercase().ends_with(".ipynb") {
                        file.size / 8
                    } else {
                        file.size
                    };

                    let stretches = (((characters + CodeIndexSize::CHARACTERS_PER_STRETCH - 1)
                        / CodeIndexSize::CHARACTERS_PER_STRETCH)
                        .max(1)) as usize;

                    estimate.code_stretches += stretches;
                    estimate.seconds += (stretches as f64) * costs.code_stretch;
                    estimate.bytes += (characters as f64) * CodeIndexSize::PER_CHARACTER
                        + (stretches as f64) * CodeIndexSize::PER_STRETCH;
                }
            }

            let delta = estimate.seconds - before;
            *estimate.kind_seconds.entry(file.kind).or_insert(0.0) += delta;
        }

        estimate
    }

    /// Estimates for each subfolder one level down, and for the files directly in `folder` (under its own path).
    pub fn estimates_by_subfolder(
        candidates: &[Candidate],
        folder: &str,
        options: &IndexOptions,
        custom_costs: Option<&Costs>,
    ) -> HashMap<String, Estimate> {
        let norm_folder = folder.replace('\\', "/");
        let prefix = if norm_folder.ends_with('/') {
            norm_folder.clone()
        } else {
            format!("{}/", norm_folder)
        };

        let mut groups: HashMap<String, Vec<Candidate>> = HashMap::new();

        for file in candidates {
            let norm_file = file.path.replace('\\', "/");
            let key = if norm_file.starts_with(&prefix) {
                let rest = &norm_file[prefix.len()..];
                if let Some(slash_idx) = rest.find('/') {
                    format!("{}{}", prefix, &rest[..slash_idx])
                } else {
                    norm_folder.clone()
                }
            } else {
                norm_folder.clone()
            };

            groups.entry(key).or_default().push(file.clone());
        }

        let mut result = HashMap::new();
        for (key, group_candidates) in groups {
            result.insert(key, Self::estimate(&group_candidates, options, custom_costs));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimator_projections() {
        let candidates = vec![
            Candidate {
                path: "/photos/cat.jpg".to_string(),
                kind: FileKind::Image,
                size: 2_000_000,
                mtime: 100.0,
                inode: 1,
                device: 1,
            },
            Candidate {
                path: "/docs/report.pdf".to_string(),
                kind: FileKind::Pdf,
                size: 500_000, // ~10 pages
                mtime: 100.0,
                inode: 2,
                device: 1,
            },
            Candidate {
                path: "/src/main.rs".to_string(),
                kind: FileKind::Code,
                size: 2800, // ~2 stretches
                mtime: 100.0,
                inode: 3,
                device: 1,
            },
        ];

        let options = IndexOptions::default();
        let est = Estimator::estimate(&candidates, &options, None);

        assert_eq!(est.files.get(&FileKind::Image), Some(&1));
        assert_eq!(est.files.get(&FileKind::Pdf), Some(&1));
        assert_eq!(est.files.get(&FileKind::Code), Some(&1));

        assert!(est.seconds > 0.0);
        assert!(est.bytes > 0.0);
    }
}
