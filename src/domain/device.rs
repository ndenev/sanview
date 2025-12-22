use crate::collectors::ZfsDriveInfo;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct PhysicalDisk {
    pub device_name: String,
    pub rank: Option<u32>,                // GEOM rank (1 = physical, higher = derived)
    pub ident: Option<String>,            // GEOM-provided identifier (WWN, serial, etc.)
    pub multipath_parent: Option<String>, // Parent multipath device (e.g., "multipath/2MVULJ1A")
    pub slot: Option<usize>,              // Physical enclosure slot number
    pub enclosure: Option<String>,        // Enclosure identifier (e.g., "ses0")
    pub statistics: DiskStatistics,
    pub path_state: PathState,
}

#[derive(Clone, Debug)]
pub struct MultipathDevice {
    pub name: String,                     // "multipath/2MVULJ1A"
    pub ident: Option<String>,            // GEOM identifier of the underlying disk
    pub state: MultipathState,            // OPTIMAL, DEGRADED, FAILED
    pub paths: Vec<String>,               // ["da0", "da1"]
    pub active_path: Option<String>,      // Currently active path
    pub statistics: DiskStatistics,       // Aggregated statistics
    pub zfs_info: Option<ZfsDriveInfo>,   // ZFS pool/vdev/role information
    pub slot: Option<usize>,              // Physical enclosure slot number
}

#[derive(Clone, Debug, PartialEq)]
pub enum MultipathState {
    Optimal,
    Degraded,
    Failed,
    Unknown,
}

impl Default for MultipathState {
    fn default() -> Self {
        MultipathState::Unknown
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiskStatistics {
    pub read_iops: f64,
    pub write_iops: f64,
    pub read_bw_mbps: f64,
    pub write_bw_mbps: f64,
    pub read_latency_ms: f64,
    pub write_latency_ms: f64,
    pub queue_depth: f64,
    pub busy_pct: f64,
    pub timestamp: Option<Instant>,
}

impl DiskStatistics {
    pub fn total_iops(&self) -> f64 {
        self.read_iops + self.write_iops
    }

    pub fn total_bw_mbps(&self) -> f64 {
        self.read_bw_mbps + self.write_bw_mbps
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathState {
    Active,
    Passive,
    Failed,
    Unknown,
}

impl Default for PathState {
    fn default() -> Self {
        PathState::Unknown
    }
}
