use crate::domain::device::{DiskStatistics, PathState, PhysicalDisk};
use anyhow::{Context, Result};
use freebsd_libgeom::{Snapshot, Statistics, Tree};
use log::debug;
use std::time::Instant;

fn is_physical_disk(name: &str) -> bool {
    name.starts_with("da") || name.starts_with("nda") || name.starts_with("multipath/")
}

pub struct GeomCollector {
    previous_snapshot: Option<Snapshot>,
    tree: Tree,
}

impl GeomCollector {
    pub fn new() -> Result<Self> {
        let tree = Tree::new()
            .context("Failed to create GEOM tree")?;

        Ok(Self {
            previous_snapshot: None,
            tree,
        })
    }

    pub fn collect(&mut self) -> Result<Vec<PhysicalDisk>> {
        let mut current_snapshot = Snapshot::new()
            .context("Failed to create GEOM snapshot")?;

        let disks = self.compute_statistics(&mut current_snapshot)?;

        self.previous_snapshot = Some(current_snapshot);
        Ok(disks)
    }

    fn compute_statistics(&mut self, current: &mut Snapshot) -> Result<Vec<PhysicalDisk>> {
        let mut disks = Vec::new();
        let timestamp = Instant::now();

        let etime = if let Some(ref mut prev) = self.previous_snapshot {
            f64::from(current.timestamp() - prev.timestamp())
        } else {
            debug!("First snapshot, no statistics available yet");
            return Ok(vec![]);
        };

        if etime <= 0.0 {
            return Ok(vec![]);
        }

        for (curstat, prevstat) in current.iter_pair(self.previous_snapshot.as_mut()) {
            if let Some(gident) = self.tree.lookup(curstat.id()) {
                // Get rank - physical devices are typically rank 1
                let rank = gident.rank();

                if let Ok(name_cstr) = gident.name() {
                    let device_name = name_cstr.to_string_lossy().to_string();

                    // Filter: only keep physical disks (da*, nda*) or multipath devices
                    if !is_physical_disk(&device_name) {
                        continue;
                    }

                    // Filter: skip derived devices (partitions, etc.) - only keep rank 1 or multipath
                    // Multipath devices may not have rank or have different ranks
                    if let Some(r) = rank {
                        if r > 1 && !device_name.starts_with("multipath/") {
                            debug!("Skipping derived device {} (rank {})", device_name, r);
                            continue;
                        }
                    }

                    let stats_computed = Statistics::compute(curstat, prevstat, etime);

                    let stats = DiskStatistics {
                        read_iops: stats_computed.transfers_per_second_read(),
                        write_iops: stats_computed.transfers_per_second_write(),
                        read_bw_mbps: stats_computed.mb_per_second_read(),
                        write_bw_mbps: stats_computed.mb_per_second_write(),
                        read_latency_ms: stats_computed.ms_per_transaction_read(),
                        write_latency_ms: stats_computed.ms_per_transaction_write(),
                        queue_depth: stats_computed.queue_length() as f64,
                        busy_pct: stats_computed.busy_pct(),
                        timestamp: Some(timestamp),
                    };

                    if stats.total_iops() > 0.1 || stats.busy_pct > 0.1 {
                        debug!(
                            "{} (rank {:?}): {:.1} IOPS, {:.1} MB/s, {:.1}% busy",
                            device_name,
                            rank,
                            stats.total_iops(),
                            stats.total_bw_mbps(),
                            stats.busy_pct
                        );
                    }

                    disks.push(PhysicalDisk {
                        device_name,
                        rank,
                        ident: None,  // Populated by topology correlator
                        multipath_parent: None,
                        slot: None,   // Populated by topology correlator from SES
                        enclosure: None,
                        statistics: stats,
                        path_state: PathState::Unknown,
                    });
                }
            }
        }

        Ok(disks)
    }
}

impl Default for GeomCollector {
    fn default() -> Self {
        Self::new().expect("Failed to create GeomCollector")
    }
}
