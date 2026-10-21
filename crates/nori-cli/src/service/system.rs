//! System CPU and processor load sampling for speed checks and indexing throttling.

use std::time::Instant;

/// System CPU load sample.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub busy: f64,
    pub total: f64,
    pub own: f64,
    pub timestamp: Instant,
}

impl Sample {
    /// Percentage of processor busy time (0.0 to 1.0) between two samples.
    pub fn load_between(&self, earlier: &Sample) -> f64 {
        let total_diff = self.total - earlier.total;
        if total_diff <= 0.0 {
            0.0
        } else {
            ((self.busy - earlier.busy) / total_diff).clamp(0.0, 1.0)
        }
    }
}

pub struct SystemLoad;

impl SystemLoad {
    /// Take a sample of the system's processor load.
    pub fn sample() -> Option<Sample> {
        let now = Instant::now();

        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::FILETIME;
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, GetProcessTimes, GetSystemTimes,
            };

            unsafe {
                let mut idle = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut kernel = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut user = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut created = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut exited = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut own_kernel = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let mut own_user = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };

                if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
                    return None;
                }
                if GetProcessTimes(
                    GetCurrentProcess(),
                    &mut created,
                    &mut exited,
                    &mut own_kernel,
                    &mut own_user,
                ) == 0 {
                    return None;
                }

                let ft_to_u64 = |ft: FILETIME| -> u64 {
                    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
                };

                let kernel_val = ft_to_u64(kernel) as f64;
                let user_val = ft_to_u64(user) as f64;
                let idle_val = ft_to_u64(idle) as f64;
                let total = kernel_val + user_val;
                let busy = total - idle_val;
                let own = (ft_to_u64(own_kernel) + ft_to_u64(own_user)) as f64 / 1e7;

                Some(Sample {
                    busy,
                    total,
                    own,
                    timestamp: now,
                })
            }
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
                if let Some(line) = stat.lines().next() {
                    if line.starts_with("cpu ") {
                        let fields: Vec<f64> = line
                            .split_whitespace()
                            .skip(1)
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if fields.len() >= 5 {
                            let idle = fields[3] + fields[4];
                            let busy = fields.iter().take(8).sum::<f64>() - idle;
                            return Some(Sample {
                                busy,
                                total: busy + idle,
                                own: 0.0,
                                timestamp: now,
                            });
                        }
                    }
                }
            }
            None
        }

        #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
        {
            None
        }
    }
}
