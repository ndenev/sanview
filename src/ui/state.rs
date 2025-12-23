use crate::collectors::{CpuStats, JailInfo, MemoryStats, NetworkStats, VmInfo};
use crate::domain::device::{MultipathDevice, PhysicalDisk};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Minimum history size to ensure some data is always available
const MIN_HISTORY_SIZE: usize = 60;

#[derive(Clone, Debug)]
pub struct AppState {
    pub multipath_devices: Vec<MultipathDevice>,
    pub standalone_disks: Vec<PhysicalDisk>,
    pub cpu_stats: Option<CpuStats>,
    pub memory_stats: Option<MemoryStats>,
    pub network_stats: Vec<NetworkStats>,
    pub vms: Vec<VmInfo>,
    pub jails: Vec<JailInfo>,
    pub last_update: Instant,
    pub should_quit: bool,

    // Dynamic history size based on terminal width
    history_size: usize,

    // Historical data for sparklines
    pub cpu_history: Vec<VecDeque<f64>>,  // Per-core history
    pub cpu_aggregate_history: VecDeque<f64>,  // Aggregate CPU utilization %
    pub memory_history: VecDeque<f64>,     // Memory usage % history
    pub arc_size_history: VecDeque<f64>,   // ARC size in GB
    pub arc_ratio_history: VecDeque<f64>,  // Compression ratio

    // Storage aggregate history (from multipath devices only - no double counting)
    pub storage_read_iops_history: VecDeque<f64>,   // Read IOPS
    pub storage_write_iops_history: VecDeque<f64>,  // Write IOPS
    pub storage_read_bw_history: VecDeque<f64>,     // Read MB/s
    pub storage_write_bw_history: VecDeque<f64>,    // Write MB/s
    pub storage_read_latency_history: VecDeque<f64>,  // Read latency ms
    pub storage_write_latency_history: VecDeque<f64>, // Write latency ms
    pub storage_queue_depth_history: VecDeque<f64>,   // Queue depth
    pub storage_busy_history: VecDeque<f64>,        // Avg busy %

    // Per-drive busy % history for individual sparklines
    pub drive_busy_history: HashMap<String, VecDeque<f64>>,

    // Network interface history (combined RX+TX bytes/sec)
    pub network_history: HashMap<String, VecDeque<f64>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            multipath_devices: Vec::new(),
            standalone_disks: Vec::new(),
            cpu_stats: None,
            memory_stats: None,
            network_stats: Vec::new(),
            vms: Vec::new(),
            jails: Vec::new(),
            last_update: Instant::now(),
            should_quit: false,
            history_size: MIN_HISTORY_SIZE,
            cpu_history: Vec::new(),
            cpu_aggregate_history: VecDeque::new(),
            memory_history: VecDeque::new(),
            arc_size_history: VecDeque::new(),
            arc_ratio_history: VecDeque::new(),
            storage_read_iops_history: VecDeque::new(),
            storage_write_iops_history: VecDeque::new(),
            storage_read_bw_history: VecDeque::new(),
            storage_write_bw_history: VecDeque::new(),
            storage_read_latency_history: VecDeque::new(),
            storage_write_latency_history: VecDeque::new(),
            storage_queue_depth_history: VecDeque::new(),
            storage_busy_history: VecDeque::new(),
            drive_busy_history: HashMap::new(),
            network_history: HashMap::new(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update history size based on terminal width
    /// Pre-fills storage history buffers with zeros on first call so charts scroll from start
    pub fn set_terminal_width(&mut self, width: u16) {
        let new_size = (width as usize * 2).max(MIN_HISTORY_SIZE); // *2 for braille resolution

        // Pre-fill histories if they're empty (first call) so charts scroll from start
        if self.storage_read_iops_history.is_empty() {
            self.storage_read_iops_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_write_iops_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_read_bw_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_write_bw_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_read_latency_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_write_latency_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_queue_depth_history = VecDeque::from(vec![0.0; new_size]);
            self.storage_busy_history = VecDeque::from(vec![0.0; new_size]);
        }

        // Pre-fill CPU aggregate history
        if self.cpu_aggregate_history.is_empty() {
            self.cpu_aggregate_history = VecDeque::from(vec![0.0; new_size]);
        }

        self.history_size = new_size;
    }

    fn trim_history<T>(history: &mut VecDeque<T>, max_size: usize) {
        while history.len() > max_size {
            history.pop_front();
        }
    }

    pub fn update_topology(
        &mut self,
        multipath_devices: Vec<MultipathDevice>,
        standalone_disks: Vec<PhysicalDisk>,
    ) {
        let history_size = self.history_size;

        // Calculate aggregate stats from multipath devices only (no double counting)
        let total_read_iops: f64 = multipath_devices.iter().map(|d| d.statistics.read_iops).sum();
        let total_write_iops: f64 = multipath_devices.iter().map(|d| d.statistics.write_iops).sum();
        let total_read_bw: f64 = multipath_devices.iter().map(|d| d.statistics.read_bw_mbps).sum();
        let total_write_bw: f64 = multipath_devices.iter().map(|d| d.statistics.write_bw_mbps).sum();

        // Average latency (weighted by IOPS would be better, but simple avg for now)
        let (avg_read_latency, avg_write_latency) = if !multipath_devices.is_empty() {
            let active_read: Vec<_> = multipath_devices.iter()
                .filter(|d| d.statistics.read_iops > 0.1)
                .collect();
            let active_write: Vec<_> = multipath_devices.iter()
                .filter(|d| d.statistics.write_iops > 0.1)
                .collect();

            let read_lat = if !active_read.is_empty() {
                active_read.iter().map(|d| d.statistics.read_latency_ms).sum::<f64>() / active_read.len() as f64
            } else {
                0.0
            };
            let write_lat = if !active_write.is_empty() {
                active_write.iter().map(|d| d.statistics.write_latency_ms).sum::<f64>() / active_write.len() as f64
            } else {
                0.0
            };
            (read_lat, write_lat)
        } else {
            (0.0, 0.0)
        };

        // Sum queue depths
        let total_queue_depth: f64 = multipath_devices.iter().map(|d| d.statistics.queue_depth).sum();

        let avg_busy: f64 = if !multipath_devices.is_empty() {
            multipath_devices.iter().map(|d| d.statistics.busy_pct).sum::<f64>() / multipath_devices.len() as f64
        } else {
            0.0
        };

        // Update storage history
        self.storage_read_iops_history.push_back(total_read_iops);
        Self::trim_history(&mut self.storage_read_iops_history, history_size);

        self.storage_write_iops_history.push_back(total_write_iops);
        Self::trim_history(&mut self.storage_write_iops_history, history_size);

        self.storage_read_bw_history.push_back(total_read_bw);
        Self::trim_history(&mut self.storage_read_bw_history, history_size);

        self.storage_write_bw_history.push_back(total_write_bw);
        Self::trim_history(&mut self.storage_write_bw_history, history_size);

        self.storage_read_latency_history.push_back(avg_read_latency);
        Self::trim_history(&mut self.storage_read_latency_history, history_size);

        self.storage_write_latency_history.push_back(avg_write_latency);
        Self::trim_history(&mut self.storage_write_latency_history, history_size);

        self.storage_queue_depth_history.push_back(total_queue_depth);
        Self::trim_history(&mut self.storage_queue_depth_history, history_size);

        self.storage_busy_history.push_back(avg_busy);
        Self::trim_history(&mut self.storage_busy_history, history_size);

        // Update per-drive busy % history
        for device in &multipath_devices {
            let history = self.drive_busy_history
                .entry(device.name.clone())
                .or_insert_with(|| {
                    // Pre-fill with zeros so sparkline scrolls from start
                    VecDeque::from(vec![0.0; history_size])
                });

            history.push_back(device.statistics.busy_pct);
            Self::trim_history(history, history_size);
        }

        // Clean up history for devices that no longer exist
        self.drive_busy_history.retain(|name, _| {
            multipath_devices.iter().any(|d| &d.name == name)
        });

        self.multipath_devices = multipath_devices;
        self.standalone_disks = standalone_disks;
        self.last_update = Instant::now();
    }

    pub fn update_system_stats(
        &mut self,
        cpu_stats: CpuStats,
        memory_stats: MemoryStats,
        network_stats: Vec<NetworkStats>,
        vms: Vec<VmInfo>,
        jails: Vec<JailInfo>,
    ) {
        let history_size = self.history_size;

        // Initialize CPU history if needed
        if self.cpu_history.len() != cpu_stats.cores.len() {
            self.cpu_history = vec![VecDeque::new(); cpu_stats.cores.len()];
        }

        // Update CPU history
        for (i, core) in cpu_stats.cores.iter().enumerate() {
            if let Some(history) = self.cpu_history.get_mut(i) {
                history.push_back(core.total_pct);
                Self::trim_history(history, history_size);
            }
        }

        // Update aggregate CPU history (average of all cores)
        let avg_cpu = if !cpu_stats.cores.is_empty() {
            cpu_stats.cores.iter().map(|c| c.total_pct).sum::<f64>() / cpu_stats.cores.len() as f64
        } else {
            0.0
        };
        self.cpu_aggregate_history.push_back(avg_cpu);
        Self::trim_history(&mut self.cpu_aggregate_history, history_size);

        // Update memory history
        self.memory_history.push_back(memory_stats.used_pct);
        Self::trim_history(&mut self.memory_history, history_size);

        // Update ARC history
        let arc_size_gb = memory_stats.arc_total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        self.arc_size_history.push_back(arc_size_gb);
        Self::trim_history(&mut self.arc_size_history, history_size);

        self.arc_ratio_history.push_back(memory_stats.arc_ratio);
        Self::trim_history(&mut self.arc_ratio_history, history_size);

        // Update network history (combined RX+TX for each interface)
        // Use raw (non-smoothed) values for the chart to show actual traffic pattern
        for iface in &network_stats {
            let total_bw_raw = iface.rx_bytes_per_sec_raw + iface.tx_bytes_per_sec_raw;
            let history = self.network_history
                .entry(iface.name.clone())
                .or_insert_with(|| {
                    // Pre-fill with zeros so chart scrolls from start
                    VecDeque::from(vec![0.0; history_size])
                });
            history.push_back(total_bw_raw);
            Self::trim_history(history, history_size);
        }

        // Clean up history for interfaces that no longer exist
        let current_ifaces: std::collections::HashSet<String> = network_stats.iter()
            .map(|i| i.name.clone())
            .collect();
        self.network_history.retain(|name, _| current_ifaces.contains(name));

        self.cpu_stats = Some(cpu_stats);
        self.memory_stats = Some(memory_stats);
        self.network_stats = network_stats;
        self.vms = vms;
        self.jails = jails;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }
}
