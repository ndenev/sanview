use anyhow::{Context, Result};
use sysctl::Sysctl;

#[derive(Clone, Debug)]
pub struct MemoryStats {
    pub total_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub laundry_bytes: u64,
    pub wired_bytes: u64,
    pub buf_bytes: u64,
    pub free_bytes: u64,
    pub used_pct: f64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_used_pct: f64,

    // ZFS ARC stats
    pub arc_total_bytes: u64,
    pub arc_mfu_bytes: u64,
    pub arc_mru_bytes: u64,
    pub arc_anon_bytes: u64,
    pub arc_header_bytes: u64,
    pub arc_other_bytes: u64,
    pub arc_compressed_bytes: u64,
    pub arc_uncompressed_bytes: u64,
    pub arc_ratio: f64,
}

pub struct MemoryCollector;

impl MemoryCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn collect(&self) -> Result<MemoryStats> {
        let page_size = sysctl_u64("hw.pagesize")?;

        let total_pages = sysctl_u64("vm.stats.vm.v_page_count")?;
        let active_pages = sysctl_u64("vm.stats.vm.v_active_count")?;
        let inactive_pages = sysctl_u64("vm.stats.vm.v_inactive_count")?;
        let laundry_pages = sysctl_u64("vm.stats.vm.v_laundry_count").unwrap_or(0);
        let wired_pages = sysctl_u64("vm.stats.vm.v_wire_count")?;
        let free_pages = sysctl_u64("vm.stats.vm.v_free_count")?;

        let total_bytes = total_pages * page_size;
        let active_bytes = active_pages * page_size;
        let inactive_bytes = inactive_pages * page_size;
        let laundry_bytes = laundry_pages * page_size;
        let wired_bytes = wired_pages * page_size;
        let buf_bytes = sysctl_u64("vfs.bufspace").unwrap_or(0);
        let free_bytes = free_pages * page_size;

        let used_bytes = total_bytes - free_bytes;
        let used_pct = if total_bytes > 0 {
            (used_bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };

        // Swap statistics
        let swap_total_bytes = sysctl_u64("vm.swap_total").unwrap_or(0);
        let swap_used_bytes = if swap_total_bytes > 0 {
            let swap_free = sysctl_u64("vm.stats.vm.v_swappgsfree").unwrap_or(0) * page_size;
            swap_total_bytes.saturating_sub(swap_free)
        } else {
            0
        };

        let swap_used_pct = if swap_total_bytes > 0 {
            (swap_used_bytes as f64 / swap_total_bytes as f64) * 100.0
        } else {
            0.0
        };

        // ZFS ARC statistics
        let arc_total_bytes = sysctl_u64("kstat.zfs.misc.arcstats.size").unwrap_or(0);
        let arc_mfu_bytes = sysctl_u64("kstat.zfs.misc.arcstats.mfu_size").unwrap_or(0);
        let arc_mru_bytes = sysctl_u64("kstat.zfs.misc.arcstats.mru_size").unwrap_or(0);
        let arc_anon_bytes = sysctl_u64("kstat.zfs.misc.arcstats.anon_size").unwrap_or(0);
        let arc_header_bytes = sysctl_u64("kstat.zfs.misc.arcstats.hdr_size").unwrap_or(0);
        let arc_other_bytes = sysctl_u64("kstat.zfs.misc.arcstats.other_size").unwrap_or(0);
        let arc_compressed_bytes = sysctl_u64("kstat.zfs.misc.arcstats.compressed_size").unwrap_or(0);
        let arc_uncompressed_bytes = sysctl_u64("kstat.zfs.misc.arcstats.uncompressed_size").unwrap_or(0);

        let arc_ratio = if arc_compressed_bytes > 0 {
            arc_uncompressed_bytes as f64 / arc_compressed_bytes as f64
        } else {
            1.0
        };

        Ok(MemoryStats {
            total_bytes,
            active_bytes,
            inactive_bytes,
            laundry_bytes,
            wired_bytes,
            buf_bytes,
            free_bytes,
            used_pct,
            swap_total_bytes,
            swap_used_bytes,
            swap_used_pct,
            arc_total_bytes,
            arc_mfu_bytes,
            arc_mru_bytes,
            arc_anon_bytes,
            arc_header_bytes,
            arc_other_bytes,
            arc_compressed_bytes,
            arc_uncompressed_bytes,
            arc_ratio,
        })
    }
}

impl Default for MemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Read a sysctl value as u64 using the sysctl crate (safe)
fn sysctl_u64(name: &str) -> Result<u64> {
    let ctl = sysctl::Ctl::new(name)
        .with_context(|| format!("Failed to access sysctl {}", name))?;

    let val = ctl.value()
        .with_context(|| format!("Failed to read sysctl {}", name))?;

    // Handle different sysctl value types
    match val {
        sysctl::CtlValue::U64(v) => Ok(v),
        sysctl::CtlValue::S64(v) => Ok(v as u64),
        sysctl::CtlValue::U32(v) => Ok(v as u64),
        sysctl::CtlValue::S32(v) => Ok(v as u64),
        sysctl::CtlValue::Int(v) => Ok(v as u64),
        sysctl::CtlValue::Uint(v) => Ok(v as u64),
        sysctl::CtlValue::Long(v) => Ok(v as u64),
        sysctl::CtlValue::Ulong(v) => Ok(v as u64),
        _ => anyhow::bail!("Unexpected sysctl type for {}: {:?}", name, val),
    }
}
