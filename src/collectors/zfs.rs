use anyhow::Result;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZfsRole {
    Data,
    Slog,
    Cache,
    Spare,
}

#[derive(Debug, Clone)]
pub struct ZfsDriveInfo {
    pub pool: String,
    pub vdev: String,
    pub role: ZfsRole,
    pub state: String,
}

/// Cache duration for ZFS topology (topology rarely changes)
const CACHE_DURATION: Duration = Duration::from_secs(30);

pub struct ZfsCollector {
    cache: Option<HashMap<String, ZfsDriveInfo>>,
    last_update: Option<Instant>,
}

impl ZfsCollector {
    pub fn new() -> Self {
        Self {
            cache: None,
            last_update: None,
        }
    }

    /// Collect ZFS topology information for all pools
    /// Returns a map of device name -> ZFS info
    /// Results are cached for 30 seconds since topology rarely changes
    pub fn collect(&mut self) -> Result<HashMap<String, ZfsDriveInfo>> {
        // Return cached result if still valid
        if let (Some(ref cache), Some(last_update)) = (&self.cache, self.last_update) {
            if last_update.elapsed() < CACHE_DURATION {
                return Ok(cache.clone());
            }
        }

        // Refresh cache
        let mut drive_map = HashMap::new();

        // Get list of all pools
        let pools = self.get_pools()?;

        // Parse each pool's status
        for pool in pools {
            let pool_info = self.parse_pool_status(&pool)?;
            drive_map.extend(pool_info);
        }

        self.cache = Some(drive_map.clone());
        self.last_update = Some(Instant::now());

        Ok(drive_map)
    }

    fn get_pools(&self) -> Result<Vec<String>> {
        let output = Command::new("zpool")
            .arg("list")
            .arg("-H")
            .arg("-o")
            .arg("name")
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    }

    fn parse_pool_status(&self, pool: &str) -> Result<HashMap<String, ZfsDriveInfo>> {
        let output = Command::new("zpool")
            .arg("status")
            .arg(pool)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut drive_map = HashMap::new();

        let mut current_role = ZfsRole::Data;
        let mut current_vdev = String::new();
        let mut in_config = false;

        for line in stdout.lines() {
            let trimmed = line.trim_start();

            // Skip until we reach config section
            if trimmed.starts_with("config:") {
                in_config = true;
                continue;
            }

            if !in_config {
                continue;
            }

            // Stop at errors section
            if trimmed.starts_with("errors:") {
                break;
            }

            // Skip header lines
            if trimmed.starts_with("NAME") || trimmed.starts_with(pool) {
                continue;
            }

            // Check for role sections - reset vdev when entering new section
            // Use first word only to handle trailing whitespace
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            if first_word == "logs" {
                current_role = ZfsRole::Slog;
                current_vdev = String::new();
                continue;
            } else if first_word == "cache" {
                current_role = ZfsRole::Cache;
                current_vdev = String::new();
                continue;
            } else if first_word == "spares" {
                current_role = ZfsRole::Spare;
                current_vdev = String::new();
                continue;
            }

            // Parse device lines
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let device_name = parts[0];
            let state = parts[1].to_string();

            // Track vdev names (raidz1-0, mirror-5, etc.)
            if device_name.starts_with("raidz") || device_name.starts_with("mirror") {
                current_vdev = device_name.to_string();
                continue;
            }

            // Skip if not a multipath device
            if !device_name.starts_with("multipath/") {
                continue;
            }

            // Extract base device name (remove partition suffix if present)
            let base_name = if let Some(idx) = device_name.rfind('p') {
                // Check if what follows 'p' is a number (partition)
                let after_p = &device_name[idx + 1..];
                if after_p.chars().all(|c| c.is_ascii_digit()) {
                    &device_name[..idx]
                } else {
                    device_name
                }
            } else {
                device_name
            };

            drive_map.insert(
                base_name.to_string(),
                ZfsDriveInfo {
                    pool: pool.to_string(),
                    vdev: current_vdev.clone(),
                    role: current_role.clone(),
                    state,
                },
            );
        }

        Ok(drive_map)
    }
}

impl Default for ZfsCollector {
    fn default() -> Self {
        Self::new()
    }
}
