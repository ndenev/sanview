use anyhow::Result;
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
        // kern.cp_times returns an array of c_long values (5 per CPU core)
        // The sysctl crate cannot handle array-type sysctls (see github.com/johalun/sysctl-rs/issues/26)
        // so we use direct sysctlbyname calls here
        let name = CString::new("kern.cp_times")?;

        // First call to get required buffer size
        let mut size: libc::size_t = 0;
        // SAFETY: sysctlbyname with null buffer is safe and returns required size
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
            anyhow::bail!("sysctlbyname kern.cp_times size query failed");
        }

        // Allocate buffer and retrieve data
        let mut buffer: Vec<u8> = vec![0; size];
        // SAFETY: buffer is correctly sized from previous sysctlbyname call
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
            anyhow::bail!("sysctlbyname kern.cp_times data query failed");
        }

        // Parse the raw bytes as c_long array (8 bytes each on 64-bit FreeBSD)
        let long_size = std::mem::size_of::<libc::c_long>();
        let num_longs = size / long_size;

        let mut values: Vec<u64> = Vec::with_capacity(num_longs);
        for i in 0..num_longs {
            let offset = i * long_size;
            let bytes = &buffer[offset..offset + long_size];
            let value = libc::c_long::from_ne_bytes(bytes.try_into().unwrap());
            values.push(value as u64);
        }

        // Group into CPU times: 5 values per core (user, nice, system, interrupt, idle)
        let mut cpu_times = Vec::new();
        for chunk in values.chunks(5) {
            if chunk.len() == 5 {
                cpu_times.push(CpuTime {
                    user: chunk[0],
                    nice: chunk[1],
                    system: chunk[2],
                    interrupt: chunk[3],
                    idle: chunk[4],
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
