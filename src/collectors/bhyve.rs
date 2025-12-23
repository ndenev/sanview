use anyhow::Result;
use libc::{c_int, c_void, size_t};
use log::debug;
use nix::unistd::sysconf;
use nix::unistd::SysconfVar;
use std::collections::HashMap;
use std::mem;

// FreeBSD sysctl MIB values
const CTL_KERN: c_int = 1;
const KERN_PROC: c_int = 14;
const KERN_PROC_ALL: c_int = 0;
const KERN_PROC_ARGS: c_int = 7;

// Fixed-point to float conversion for ki_pctcpu
// FreeBSD uses FSCALE = 2048 for fixpt_t
const FSCALE: f64 = 2048.0;

fn fixpt_to_pct(fixpt: u32) -> f64 {
    (fixpt as f64 / FSCALE) * 100.0
}

#[derive(Clone, Debug)]
pub struct VmInfo {
    pub name: String,
    pub pid: u32,
    pub cpu_pct: f64,           // CPU percentage (sum of all threads)
    pub memory_bytes: u64,      // Resident memory in bytes
    pub virtual_bytes: u64,     // Virtual memory in bytes
    pub runtime_secs: f64,      // Total runtime in seconds
}

pub struct BhyveCollector {
    page_size: usize,
}

impl BhyveCollector {
    pub fn new() -> Self {
        // Use nix for safe sysconf access
        let page_size = sysconf(SysconfVar::PAGE_SIZE)
            .ok()
            .flatten()
            .map(|v| v as usize)
            .unwrap_or(4096);

        Self { page_size }
    }

    pub fn collect(&self) -> Result<Vec<VmInfo>> {
        let mut vms = self.get_bhyve_vms()?;

        // Sort by memory usage (descending)
        vms.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));

        debug!("Found {} bhyve VMs", vms.len());
        Ok(vms)
    }

    /// Get the process title (argv[0]) for a given PID using KERN_PROC_ARGS
    fn get_proc_args(&self, pid: i32) -> Option<String> {
        let mib: [c_int; 4] = [CTL_KERN, KERN_PROC, KERN_PROC_ARGS, pid];
        let mut size: size_t = 0;

        // SAFETY: sysctl is a standard FreeBSD system call
        // First call with null buffer to get required size
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr(),
                4,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null(),
                0,
            )
        };
        if ret != 0 || size == 0 {
            return None;
        }

        let mut buffer: Vec<u8> = vec![0; size];

        // SAFETY: buffer is properly sized from previous sysctl call
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr(),
                4,
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
                std::ptr::null(),
                0,
            )
        };
        if ret != 0 {
            return None;
        }

        // Args are null-separated; get the first one (process title)
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
    }

    fn get_bhyve_vms(&self) -> Result<Vec<VmInfo>> {
        // Build MIB for KERN_PROC_ALL (3 elements)
        let mib: [c_int; 3] = [CTL_KERN, KERN_PROC, KERN_PROC_ALL];

        // First call to get buffer size
        let mut size: size_t = 0;

        // SAFETY: sysctl is a standard FreeBSD system call
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null(),
                0,
            )
        };

        if ret != 0 {
            anyhow::bail!("sysctl KERN_PROC_ALL size query failed");
        }

        // Add some slack for new processes that may appear between calls
        size = size * 5 / 4;

        // Allocate buffer
        let kinfo_size = mem::size_of::<KinfoProc>();
        let mut buffer: Vec<u8> = vec![0; size];

        // SAFETY: buffer is properly allocated with extra slack
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr(),
                3,
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
                std::ptr::null(),
                0,
            )
        };

        if ret != 0 {
            anyhow::bail!("sysctl KERN_PROC_ALL data query failed");
        }

        // Aggregate stats by PID (bhyve has multiple threads per VM)
        let mut vm_stats: HashMap<i32, VmStats> = HashMap::new();

        let num_procs = size / kinfo_size;
        for i in 0..num_procs {
            let offset = i * kinfo_size;

            // SAFETY: We verify offset + kinfo_size <= buffer.len()
            // The kinfo_proc struct layout must match FreeBSD's exactly
            if offset + kinfo_size > buffer.len() {
                break;
            }

            let kinfo = unsafe { &*(buffer.as_ptr().add(offset) as *const KinfoProc) };

            // Extract command name
            // SAFETY: ki_comm is a null-terminated C string within the struct
            let comm = unsafe {
                std::ffi::CStr::from_ptr(kinfo.ki_comm.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };

            // Only process bhyve processes
            if comm != "bhyve" {
                continue;
            }

            let pid = kinfo.ki_pid;
            let cpu_pct = fixpt_to_pct(kinfo.ki_pctcpu);
            let memory_bytes = (kinfo.ki_rssize as u64) * (self.page_size as u64);
            let virtual_bytes = kinfo.ki_size;
            let runtime_secs = kinfo.ki_runtime as f64 / 1_000_000.0;

            let entry = vm_stats.entry(pid).or_insert(VmStats {
                cpu_pct: 0.0,
                memory_bytes: 0,
                virtual_bytes: 0,
                runtime_secs: 0.0,
            });

            // Aggregate CPU across all threads
            entry.cpu_pct += cpu_pct;
            // Memory and virtual size are shared, take max
            entry.memory_bytes = entry.memory_bytes.max(memory_bytes);
            entry.virtual_bytes = entry.virtual_bytes.max(virtual_bytes);
            // Runtime is per-thread, take max
            entry.runtime_secs = entry.runtime_secs.max(runtime_secs);
        }

        // Now get VM names for each PID using KERN_PROC_ARGS
        let mut vms = Vec::new();
        for (pid, stats) in vm_stats {
            // Get process title to extract VM name
            let name = if let Some(args) = self.get_proc_args(pid) {
                // Format is "bhyve: <vmname>"
                args.strip_prefix("bhyve: ")
                    .or_else(|| args.strip_prefix("bhyve:"))
                    .unwrap_or(&args)
                    .trim()
                    .to_string()
            } else {
                format!("pid-{}", pid)
            };

            vms.push(VmInfo {
                name,
                pid: pid as u32,
                cpu_pct: stats.cpu_pct,
                memory_bytes: stats.memory_bytes,
                virtual_bytes: stats.virtual_bytes,
                runtime_secs: stats.runtime_secs,
            });
        }

        Ok(vms)
    }
}

impl Default for BhyveCollector {
    fn default() -> Self {
        Self::new()
    }
}

struct VmStats {
    cpu_pct: f64,
    memory_bytes: u64,
    virtual_bytes: u64,
    runtime_secs: f64,
}

/// Minimal kinfo_proc structure with fields we need
/// Must match FreeBSD's struct layout exactly
///
/// WARNING: This struct layout is FreeBSD version-specific.
/// It was created for FreeBSD 14.x and may need updates for other versions.
/// See sys/user.h for the authoritative definition.
#[repr(C)]
struct KinfoProc {
    ki_structsize: i32,
    ki_layout: i32,
    ki_args: *mut c_void,
    ki_paddr: *mut c_void,
    ki_addr: *mut c_void,
    ki_tracep: *mut c_void,
    ki_textvp: *mut c_void,
    ki_fd: *mut c_void,
    ki_vmspace: *mut c_void,
    ki_wchan: *const c_void,
    ki_pid: i32,
    ki_ppid: i32,
    ki_pgid: i32,
    ki_tpgid: i32,
    ki_sid: i32,
    ki_tsid: i32,
    ki_jobc: i16,
    ki_spare_short1: i16,
    ki_tdev_freebsd11: u32,
    ki_siglist: [u32; 4],      // sigset_t
    ki_sigmask: [u32; 4],
    ki_sigignore: [u32; 4],
    ki_sigcatch: [u32; 4],
    ki_uid: u32,
    ki_ruid: u32,
    ki_svuid: u32,
    ki_rgid: u32,
    ki_svgid: u32,
    ki_ngroups: i16,
    ki_spare_short2: i16,
    ki_groups: [u32; 16],      // KI_NGROUPS
    ki_size: u64,              // vm_size_t - virtual size
    ki_rssize: i64,            // segsz_t - resident set size in pages
    ki_swrss: i64,
    ki_tsize: i64,
    ki_dsize: i64,
    ki_ssize: i64,
    ki_xstat: u16,
    ki_acflag: u16,
    ki_pctcpu: u32,            // fixpt_t - CPU percentage
    ki_estcpu: u32,
    ki_slptime: u32,
    ki_swtime: u32,
    ki_cow: u32,
    ki_runtime: u64,           // Real time in microsec
    ki_start: [i64; 2],        // struct timeval
    ki_childtime: [i64; 2],
    ki_flag: i64,
    ki_kiflag: i64,
    ki_traceflag: i32,
    ki_stat: i8,
    ki_nice: i8,
    ki_lock: i8,
    ki_rqindex: i8,
    ki_oncpu_old: u8,
    ki_lastcpu_old: u8,
    ki_tdname: [i8; 17],       // TDNAMLEN + 1
    ki_wmesg: [i8; 9],         // WMESGLEN + 1
    ki_login: [i8; 18],        // LOGNAMELEN + 1
    ki_lockname: [i8; 9],      // LOCKNAMELEN + 1
    ki_comm: [i8; 20],         // COMMLEN + 1
    ki_emul: [i8; 17],         // KI_EMULNAMELEN + 1
    ki_loginclass: [i8; 18],   // LOGINCLASSLEN + 1
    ki_moretdname: [i8; 4],
    ki_sparestrings: [i8; 46],
    ki_spareints: [i32; 2],
    ki_tdev: u64,
    ki_oncpu: i32,
    ki_lastcpu: i32,
    ki_tracer: i32,
    ki_flag2: i32,
    ki_fibnum: i32,
    ki_cr_flags: u32,
    ki_jid: i32,
    ki_numthreads: i32,
    ki_tid: i32,
    ki_pri: [i32; 1],          // struct priority
    ki_rusage: [u8; 144],      // struct rusage
    ki_rusage_ch: [u8; 144],
    ki_pcb: *mut c_void,
    ki_kstack: *mut c_void,
    ki_udata: *mut c_void,
    ki_tdaddr: *mut c_void,
    ki_pd: *mut c_void,
    ki_spareptrs: [*mut c_void; 5],
    ki_sparelongs: [i64; 12],
    ki_sflag: i64,
    ki_tdflags: i64,
}
