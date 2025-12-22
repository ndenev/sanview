use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;
use std::ffi::CStr;
use std::process::Command;

// FreeBSD if_data structure (from net/if.h)
#[repr(C)]
#[allow(non_camel_case_types)]
struct if_data {
    ifi_type: u8,
    ifi_physical: u8,
    ifi_addrlen: u8,
    ifi_hdrlen: u8,
    ifi_link_state: u8,
    ifi_vhid: u8,
    ifi_datalen: u16,
    ifi_mtu: u32,
    ifi_metric: u32,
    ifi_baudrate: u64,
    ifi_ipackets: u64,
    ifi_ierrors: u64,
    ifi_opackets: u64,
    ifi_oerrors: u64,
    ifi_collisions: u64,
    ifi_ibytes: u64,
    ifi_obytes: u64,
    ifi_imcasts: u64,
    ifi_omcasts: u64,
    ifi_iqdrops: u64,
    ifi_oqdrops: u64,
    ifi_noproto: u64,
    ifi_hwassist: u64,
    ifi_epoch: i64,
    ifi_lastchange: [u64; 2],
}

#[derive(Clone, Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub link_state: u8,
    pub mtu: u32,
    pub baudrate: u64,
    pub is_aggregate: bool,
    pub aggregate_members: Vec<String>,
    pub parent_aggregate: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkStats {
    pub name: String,
    /// Smoothed rates for display (EMA)
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub rx_packets_per_sec: f64,
    pub tx_packets_per_sec: f64,
    /// Raw instantaneous rates for charting
    pub rx_bytes_per_sec_raw: f64,
    pub tx_bytes_per_sec_raw: f64,
    pub is_aggregate: bool,
    pub is_member: bool,
    pub link_state: u8,
    pub baudrate: u64,
}

/// Smoothed rate values for EMA calculation
#[derive(Clone, Default)]
struct SmoothedRates {
    rx_bytes_per_sec: f64,
    tx_bytes_per_sec: f64,
    rx_packets_per_sec: f64,
    tx_packets_per_sec: f64,
}

pub struct NetworkCollector {
    previous: HashMap<String, NetworkInterface>,
    last_collection: std::time::Instant,
    lagg_members: HashMap<String, Vec<String>>,
    /// EMA-smoothed rates per interface (for smooth display with decay)
    smoothed: HashMap<String, SmoothedRates>,
}

/// EMA smoothing factor: 0.3 means new values contribute 30%, old values 70%
/// This provides ~3-4 sample decay time (smooth but responsive)
const EMA_ALPHA: f64 = 0.3;

impl NetworkCollector {
    pub fn new() -> Self {
        Self {
            previous: HashMap::new(),
            last_collection: std::time::Instant::now(),
            lagg_members: HashMap::new(),
            smoothed: HashMap::new(),
        }
    }

    pub fn collect(&mut self) -> Result<Vec<NetworkStats>> {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_collection).as_secs_f64();

        // Refresh lagg membership periodically (it's slow, so cache it)
        if self.lagg_members.is_empty() || elapsed > 30.0 {
            self.lagg_members = self.get_lagg_members().unwrap_or_default();
        }

        // Build reverse map: member -> aggregate
        let mut member_to_aggregate: HashMap<String, String> = HashMap::new();
        for (agg, members) in &self.lagg_members {
            for member in members {
                member_to_aggregate.insert(member.clone(), agg.clone());
            }
        }

        // Get current interface stats via getifaddrs
        let current = self.collect_interfaces(&member_to_aggregate)?;

        let mut stats = Vec::new();

        // Calculate rates with EMA smoothing
        for (name, iface) in &current {
            let is_member = iface.parent_aggregate.is_some();

            // Get or create smoothed state for this interface
            let smoothed = self.smoothed.entry(name.clone()).or_default();

            if let Some(prev) = self.previous.get(name) {
                let rx_bytes_delta = iface.rx_bytes.saturating_sub(prev.rx_bytes);
                let tx_bytes_delta = iface.tx_bytes.saturating_sub(prev.tx_bytes);
                let rx_packets_delta = iface.rx_packets.saturating_sub(prev.rx_packets);
                let tx_packets_delta = iface.tx_packets.saturating_sub(prev.tx_packets);

                // Calculate instantaneous rates
                let rx_rate = rx_bytes_delta as f64 / elapsed;
                let tx_rate = tx_bytes_delta as f64 / elapsed;
                let rx_pps = rx_packets_delta as f64 / elapsed;
                let tx_pps = tx_packets_delta as f64 / elapsed;

                // Apply EMA smoothing: new_smoothed = alpha * raw + (1 - alpha) * old_smoothed
                smoothed.rx_bytes_per_sec = EMA_ALPHA * rx_rate + (1.0 - EMA_ALPHA) * smoothed.rx_bytes_per_sec;
                smoothed.tx_bytes_per_sec = EMA_ALPHA * tx_rate + (1.0 - EMA_ALPHA) * smoothed.tx_bytes_per_sec;
                smoothed.rx_packets_per_sec = EMA_ALPHA * rx_pps + (1.0 - EMA_ALPHA) * smoothed.rx_packets_per_sec;
                smoothed.tx_packets_per_sec = EMA_ALPHA * tx_pps + (1.0 - EMA_ALPHA) * smoothed.tx_packets_per_sec;

                stats.push(NetworkStats {
                    name: name.clone(),
                    rx_bytes_per_sec: smoothed.rx_bytes_per_sec,
                    tx_bytes_per_sec: smoothed.tx_bytes_per_sec,
                    rx_packets_per_sec: smoothed.rx_packets_per_sec,
                    tx_packets_per_sec: smoothed.tx_packets_per_sec,
                    rx_bytes_per_sec_raw: rx_rate,
                    tx_bytes_per_sec_raw: tx_rate,
                    is_aggregate: iface.is_aggregate,
                    is_member,
                    link_state: iface.link_state,
                    baudrate: iface.baudrate,
                });
            } else {
                // First collection, no previous data - just return zeros
                // (smoothed values are already zero from Default)
                stats.push(NetworkStats {
                    name: name.clone(),
                    is_aggregate: iface.is_aggregate,
                    is_member,
                    link_state: iface.link_state,
                    baudrate: iface.baudrate,
                    ..Default::default()
                });
            }
        }

        self.previous = current;
        self.last_collection = now;

        // Sort: aggregates first, then their members indented, then other interfaces
        stats.sort_by(|a, b| {
            // lagg first, then physical members of lagg, then other
            let a_priority = if a.is_aggregate { 0 } else if a.is_member { 1 } else { 2 };
            let b_priority = if b.is_aggregate { 0 } else if b.is_member { 1 } else { 2 };

            match a_priority.cmp(&b_priority) {
                std::cmp::Ordering::Equal => a.name.cmp(&b.name),
                other => other,
            }
        });

        Ok(stats)
    }

    fn collect_interfaces(&self, member_to_aggregate: &HashMap<String, String>) -> Result<HashMap<String, NetworkInterface>> {
        let mut interfaces: HashMap<String, NetworkInterface> = HashMap::new();

        // Skip interfaces we don't care about
        let skip_prefixes = ["lo", "pflog", "enc", "tap", "epair", "bridge", "gif", "stf"];

        unsafe {
            let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
            if libc::getifaddrs(&mut ifap) != 0 {
                anyhow::bail!("getifaddrs failed");
            }

            let mut ifa = ifap;
            while !ifa.is_null() {
                let ifaddrs = &*ifa;

                // Get interface name
                let name = CStr::from_ptr(ifaddrs.ifa_name).to_string_lossy().into_owned();

                // Only process AF_LINK entries (which have the stats in ifa_data)
                if !ifaddrs.ifa_addr.is_null() {
                    let sa_family = (*ifaddrs.ifa_addr).sa_family as i32;

                    if sa_family == libc::AF_LINK && !ifaddrs.ifa_data.is_null() {
                        // Skip unwanted interfaces
                        if !skip_prefixes.iter().any(|p| name.starts_with(p)) {
                            let if_data = ifaddrs.ifa_data as *const if_data;
                            let data = &*if_data;

                            let is_aggregate = name.starts_with("lagg");
                            let aggregate_members = self.lagg_members.get(&name).cloned().unwrap_or_default();
                            let parent_aggregate = member_to_aggregate.get(&name).cloned();

                            debug!("Network interface {}: rx={} tx={} link_state={} baudrate={}",
                                   name, data.ifi_ibytes, data.ifi_obytes, data.ifi_link_state, data.ifi_baudrate);

                            interfaces.insert(name.clone(), NetworkInterface {
                                name,
                                rx_bytes: data.ifi_ibytes,
                                tx_bytes: data.ifi_obytes,
                                rx_packets: data.ifi_ipackets,
                                tx_packets: data.ifi_opackets,
                                rx_errors: data.ifi_ierrors,
                                tx_errors: data.ifi_oerrors,
                                link_state: data.ifi_link_state,
                                mtu: data.ifi_mtu,
                                baudrate: data.ifi_baudrate,
                                is_aggregate,
                                aggregate_members,
                                parent_aggregate,
                            });
                        }
                    }
                }

                ifa = ifaddrs.ifa_next;
            }

            libc::freeifaddrs(ifap);
        }

        Ok(interfaces)
    }

    fn get_lagg_members(&self) -> Result<HashMap<String, Vec<String>>> {
        let mut lagg_members: HashMap<String, Vec<String>> = HashMap::new();

        // Find all lagg interfaces
        let output = Command::new("ifconfig")
            .args(["-l"])
            .output()
            .context("Failed to run ifconfig -l")?;

        let ifaces = String::from_utf8(output.stdout).unwrap_or_default();
        let lagg_ifaces: Vec<&str> = ifaces.split_whitespace()
            .filter(|n| n.starts_with("lagg"))
            .collect();

        for lagg in lagg_ifaces {
            let output = Command::new("ifconfig")
                .arg(lagg)
                .output()
                .context("Failed to run ifconfig for lagg")?;

            let stdout = String::from_utf8(output.stdout).unwrap_or_default();
            let mut members = Vec::new();

            for line in stdout.lines() {
                if let Some(rest) = line.trim().strip_prefix("laggport:") {
                    if let Some(member) = rest.split_whitespace().next() {
                        members.push(member.to_string());
                    }
                }
            }

            if !members.is_empty() {
                debug!("LAGG {} members: {:?}", lagg, members);
                lagg_members.insert(lagg.to_string(), members);
            }
        }

        Ok(lagg_members)
    }
}

impl Default for NetworkCollector {
    fn default() -> Self {
        Self::new()
    }
}
