use crate::collectors::multipath::MultipathInfo;
use crate::collectors::ses::SesSlotInfo;
use crate::collectors::ZfsDriveInfo;
use crate::domain::device::{DiskStatistics, MultipathDevice, PhysicalDisk};
use log::debug;
use std::collections::HashMap;

pub struct TopologyCorrelator;

impl TopologyCorrelator {
    pub fn new() -> Self {
        Self
    }

    /// Correlate physical disks with multipath devices, SES slots, ZFS info, and deduplicate
    ///
    /// Returns:
    /// - List of multipath devices (deduplicated by GEOM multipath)
    /// - List of standalone physical disks (not part of multipath)
    pub fn correlate(
        &self,
        mut physical_disks: Vec<PhysicalDisk>,
        multipath_info: HashMap<String, MultipathInfo>,
        ses_info: HashMap<String, SesSlotInfo>,
        zfs_info: HashMap<String, ZfsDriveInfo>,
    ) -> (Vec<MultipathDevice>, Vec<PhysicalDisk>) {
        let mut multipath_devices = Vec::new();
        let mut standalone_disks = Vec::new();

        // Build a map of disk_name -> disk for quick lookup
        // Also populate SES slot information
        let mut disk_map: HashMap<String, PhysicalDisk> = physical_disks
            .drain(..)
            .map(|mut d| {
                // Add SES slot information if available
                if let Some(ses_slot) = ses_info.get(&d.device_name) {
                    d.slot = Some(ses_slot.slot);
                    d.enclosure = Some(ses_slot.enclosure.clone());
                    debug!("{} -> slot {} in {}", d.device_name, ses_slot.slot, ses_slot.enclosure);
                }
                (d.device_name.clone(), d)
            })
            .collect();

        // Process multipath devices
        for (mp_name, mp_info) in multipath_info {
            let mut path_disks = Vec::new();
            let mut active_path = None;

            // Collect disks for each path
            for path_info in &mp_info.paths {
                if let Some(disk) = disk_map.remove(&path_info.device_name) {
                    if path_info.is_active {
                        active_path = Some(path_info.device_name.clone());
                    }
                    path_disks.push(disk);
                }
            }

            // Use statistics from the active path, or first available, or default
            let stats = if path_disks.is_empty() {
                debug!("Multipath device {} has no associated physical disks in GEOM snapshot", mp_name);
                DiskStatistics::default()
            } else if let Some(ref active) = active_path {
                path_disks
                    .iter()
                    .find(|d| d.device_name == *active)
                    .map(|d| d.statistics.clone())
                    .unwrap_or_default()
            } else {
                path_disks.first().map(|d| d.statistics.clone()).unwrap_or_default()
            };

            let paths: Vec<String> = path_disks.iter().map(|d| d.device_name.clone()).collect();

            // Use the serial from the multipath info (extracted from multipath name)
            let ident = Some(mp_info.serial.clone());

            // Also update the physical disks with this serial
            for disk in &mut path_disks {
                disk.ident = ident.clone();
            }

            // Use minimum slot from all paths (for consistency with dual-controller arrays)
            // First try to get slot from path disks that were found
            let mut slot = path_disks.iter()
                .filter_map(|d| d.slot)
                .min();

            // If no slot found from path disks, look up directly from SES info using path names
            if slot.is_none() {
                slot = mp_info.paths.iter()
                    .filter_map(|p| ses_info.get(&p.device_name))
                    .map(|s| s.slot)
                    .min();
            }

            debug!(
                "Multipath device {} (serial: {}): {} paths, slot={:?}, active={:?}",
                mp_name,
                mp_info.serial,
                paths.len(),
                slot,
                active_path
            );

            // Look up ZFS info for this multipath device
            let zfs = zfs_info.get(&mp_name).cloned();

            multipath_devices.push(MultipathDevice {
                name: mp_name,
                ident,
                state: mp_info.state,
                paths,
                active_path,
                statistics: stats,
                zfs_info: zfs,
                slot,
            });
        }

        // Sort multipath devices by physical slot for consistent ordering
        multipath_devices.sort_by(|a, b| {
            match (a.slot, b.slot) {
                (Some(slot_a), Some(slot_b)) => slot_a.cmp(&slot_b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.cmp(&b.name),
            }
        });

        // Remaining disks in disk_map are standalone (not part of multipath)
        // But we still need to deduplicate by WWN
        let deduplicated_standalone = self.deduplicate_by_wwn(disk_map);
        standalone_disks.extend(deduplicated_standalone);

        debug!(
            "Topology: {} multipath devices, {} standalone disks",
            multipath_devices.len(),
            standalone_disks.len()
        );

        (multipath_devices, standalone_disks)
    }

    /// Deduplicate standalone disks by identifier (WWN, serial, GEOM ident)
    /// If multiple disks have the same identifier, they're the same physical disk through different paths
    fn deduplicate_by_wwn(&self, disk_map: HashMap<String, PhysicalDisk>) -> Vec<PhysicalDisk> {
        let mut ident_groups: HashMap<String, Vec<PhysicalDisk>> = HashMap::new();
        let mut no_ident_disks = Vec::new();

        for (_, disk) in disk_map {
            if let Some(ref ident) = disk.ident {
                ident_groups.entry(ident.clone()).or_default().push(disk);
            } else {
                // No identifier, treat as unique
                no_ident_disks.push(disk);
            }
        }

        let mut result = Vec::new();

        // For each identifier group, keep only one disk (the first one)
        for (ident, mut disks) in ident_groups {
            if disks.len() > 1 {
                debug!(
                    "Deduplicating {} disks with identifier {}: {:?}",
                    disks.len(),
                    ident,
                    disks.iter().map(|d| &d.device_name).collect::<Vec<_>>()
                );
                // TODO: We could aggregate stats here if needed
            }
            result.push(disks.remove(0));
        }

        result.extend(no_ident_disks);
        result
    }
}

impl Default for TopologyCorrelator {
    fn default() -> Self {
        Self::new()
    }
}
