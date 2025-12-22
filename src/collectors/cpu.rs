use anyhow::{Context, Result};
use std::ffi::CString;

#[derive(Clone, Debug)]
pub struct CpuStats {
    pub cores: Vec<CoreStats>,
}

#[derive(Clone, Debug)]
pub struct CoreStats {
    pub core_id: usize,
    pub user_pct: f64,
    pub system_pct: f64,
    pub idle_pct: f64,
    pub total_pct: f64,  // user + system
}

pub struct CpuCollector {
    previous_times: Option<Vec<CpuTime>>,
}

#[derive(Clone, Debug)]
struct CpuTime {
    user: u64,
    nice: u64,
    system: u64,
    interrupt: u64,
    idle: u64,
}

impl CpuCollector {
    pub fn new() -> Self {
        Self {
            previous_times: None,
        }
    }

    pub fn collect(&mut self) -> Result<CpuStats> {
        let current_times = self.read_cp_times()?;

        let cores = if let Some(ref prev_times) = self.previous_times {
            // Calculate deltas and percentages
            current_times
                .iter()
                .zip(prev_times.iter())
                .enumerate()
                .map(|(core_id, (curr, prev))| {
                    let delta_user = curr.user.saturating_sub(prev.user);
                    let delta_nice = curr.nice.saturating_sub(prev.nice);
                    let delta_system = curr.system.saturating_sub(prev.system);
                    let delta_interrupt = curr.interrupt.saturating_sub(prev.interrupt);
                    let delta_idle = curr.idle.saturating_sub(prev.idle);

                    let total = delta_user + delta_nice + delta_system + delta_interrupt + delta_idle;

                    let (user_pct, system_pct, idle_pct) = if total > 0 {
                        (
                            ((delta_user + delta_nice) as f64 / total as f64) * 100.0,
                            ((delta_system + delta_interrupt) as f64 / total as f64) * 100.0,
                            (delta_idle as f64 / total as f64) * 100.0,
                        )
                    } else {
                        (0.0, 0.0, 100.0)
                    };

                    CoreStats {
                        core_id,
                        user_pct,
                        system_pct,
                        idle_pct,
                        total_pct: user_pct + system_pct,
                    }
                })
                .collect()
        } else {
            // First collection, return zeros
            current_times
                .iter()
                .enumerate()
                .map(|(core_id, _)| CoreStats {
                    core_id,
                    user_pct: 0.0,
                    system_pct: 0.0,
                    idle_pct: 100.0,
                    total_pct: 0.0,
                })
                .collect()
        };

        self.previous_times = Some(current_times);

        Ok(CpuStats { cores })
    }

    fn read_cp_times(&self) -> Result<Vec<CpuTime>> {
        // Read kern.cp_times sysctl directly (returns array of longs, 5 per CPU)
        let name = CString::new("kern.cp_times").unwrap();

        // First, get the size needed
        let mut size: usize = 0;
        let ret = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null(),
                0,
            )
        };
        if ret != 0 {
            anyhow::bail!("Failed to get kern.cp_times size: {}", std::io::Error::last_os_error());
        }

        // Allocate buffer and read the data
        let num_longs = size / std::mem::size_of::<libc::c_long>();
        let mut buffer: Vec<libc::c_long> = vec![0; num_longs];

        let ret = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buffer.as_mut_ptr() as *mut libc::c_void,
                &mut size,
                std::ptr::null(),
                0,
            )
        };
        if ret != 0 {
            anyhow::bail!("Failed to read kern.cp_times: {}", std::io::Error::last_os_error());
        }

        // Parse: 5 values per CPU (user, nice, system, interrupt, idle)
        let mut cpu_times = Vec::new();
        for chunk in buffer.chunks(5) {
            if chunk.len() == 5 {
                cpu_times.push(CpuTime {
                    user: chunk[0] as u64,
                    nice: chunk[1] as u64,
                    system: chunk[2] as u64,
                    interrupt: chunk[3] as u64,
                    idle: chunk[4] as u64,
                });
            }
        }

        Ok(cpu_times)
    }
}

impl Default for CpuCollector {
    fn default() -> Self {
        Self::new()
    }
}
