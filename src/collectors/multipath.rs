use crate::domain::device::MultipathState;
use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct MultipathInfo {
    pub name: String,
    pub serial: String,      // Extracted from multipath name (e.g., "2MVULJ1A" from "multipath/2MVULJ1A")
    pub state: MultipathState,
    pub paths: Vec<PathInfo>,
}

#[derive(Clone, Debug)]
pub struct PathInfo {
    pub device_name: String,
    pub is_active: bool,
}

/// Cache duration for multipath topology (topology rarely changes)
const CACHE_DURATION: Duration = Duration::from_secs(30);

pub struct MultipathCollector {
    cache: Option<HashMap<String, MultipathInfo>>,
    last_update: Option<Instant>,
}

impl MultipathCollector {
    pub fn new() -> Self {
        Self {
            cache: None,
            last_update: None,
        }
    }

    /// Collect multipath topology using gmultipath list
    /// Results are cached for 30 seconds since topology rarely changes
    pub fn collect(&mut self) -> Result<HashMap<String, MultipathInfo>> {
        // Return cached result if still valid
        if let (Some(ref cache), Some(last_update)) = (&self.cache, self.last_update) {
            if last_update.elapsed() < CACHE_DURATION {
                return Ok(cache.clone());
            }
        }

        let output = self.run_gmultipath_list()
            .context("Failed to run gmultipath list")?;

        let result = self.parse_gmultipath_output(&output)?;
        self.cache = Some(result.clone());
        self.last_update = Some(Instant::now());

        Ok(result)
    }

    fn run_gmultipath_list(&self) -> Result<String> {
        use std::process::Command;

        let output = Command::new("gmultipath")
            .arg("list")
            .output()
            .context("Failed to execute gmultipath")?;

        if !output.status.success() {
            anyhow::bail!("gmultipath command failed");
        }

        Ok(String::from_utf8(output.stdout)
            .context("Failed to parse gmultipath output as UTF-8")?)
    }

    fn parse_gmultipath_output(&self, output: &str) -> Result<HashMap<String, MultipathInfo>> {
        let mut multipath_devices = HashMap::new();
        let mut current_geom: Option<String> = None;
        let mut current_state = MultipathState::Unknown;
        let mut current_paths: Vec<PathInfo> = Vec::new();
        let mut in_consumers = false;
        let mut current_consumer_name: Option<String> = None;
        let mut current_consumer_active = false;

        for line in output.lines() {
            let trimmed = line.trim();

            // New geom starts
            if let Some(name) = trimmed.strip_prefix("Geom name: ") {
                // Save previous geom if exists
                if let Some(geom_name) = current_geom.take() {
                    // Add last consumer if pending
                    if let Some(consumer_name) = current_consumer_name.take() {
                        current_paths.push(PathInfo {
                            device_name: consumer_name,
                            is_active: current_consumer_active,
                        });
                    }

                    let mp_name = format!("multipath/{}", geom_name);
                    multipath_devices.insert(
                        mp_name.clone(),
                        MultipathInfo {
                            name: mp_name,
                            serial: geom_name,
                            state: current_state.clone(),
                            paths: current_paths.clone(),
                        },
                    );
                    current_paths.clear();
                }

                current_geom = Some(name.to_string());
                current_state = MultipathState::Unknown;
                in_consumers = false;
                current_consumer_name = None;
                debug!("Found multipath geom: {}", name);
            }
            // State line
            else if let Some(state_str) = trimmed.strip_prefix("State: ") {
                if !in_consumers {
                    // This is the geom state, not consumer state
                    current_state = match state_str {
                        "OPTIMAL" => MultipathState::Optimal,
                        "DEGRADED" => MultipathState::Degraded,
                        "FAILED" => MultipathState::Failed,
                        _ => MultipathState::Unknown,
                    };
                } else if let Some(ref name) = current_consumer_name {
                    // This is consumer state
                    current_consumer_active = state_str == "ACTIVE";
                    // Save this consumer
                    current_paths.push(PathInfo {
                        device_name: name.clone(),
                        is_active: current_consumer_active,
                    });
                    current_consumer_name = None;
                }
            }
            // Consumers section starts
            else if trimmed == "Consumers:" {
                in_consumers = true;
            }
            // Providers section starts (end of consumers)
            else if trimmed == "Providers:" {
                in_consumers = false;
            }
            // Consumer name line (e.g., "1. Name: da8" or just "Name: da8")
            else if in_consumers {
                if let Some(pos) = trimmed.find("Name: ") {
                    let rest = &trimmed[pos + 6..]; // Skip "Name: "
                    // Save previous consumer if pending
                    if let Some(prev_name) = current_consumer_name.take() {
                        current_paths.push(PathInfo {
                            device_name: prev_name,
                            is_active: current_consumer_active,
                        });
                    }
                    current_consumer_name = Some(rest.to_string());
                    current_consumer_active = false;
                }
            }
        }

        // Save last geom
        if let Some(geom_name) = current_geom {
            // Add last consumer if pending
            if let Some(consumer_name) = current_consumer_name {
                current_paths.push(PathInfo {
                    device_name: consumer_name,
                    is_active: current_consumer_active,
                });
            }

            let mp_name = format!("multipath/{}", geom_name);
            multipath_devices.insert(
                mp_name.clone(),
                MultipathInfo {
                    name: mp_name,
                    serial: geom_name,
                    state: current_state,
                    paths: current_paths,
                },
            );
        }

        debug!("Found {} multipath devices", multipath_devices.len());
        Ok(multipath_devices)
    }
}

impl Default for MultipathCollector {
    fn default() -> Self {
        Self::new()
    }
}
