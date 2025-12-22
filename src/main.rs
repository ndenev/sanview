use anyhow::{Context, Result};
use clap::Parser;
use sanview::collectors::{
    BhyveCollector, CpuCollector, GeomCollector, JailCollector, MemoryCollector,
    MultipathCollector, NetworkCollector, SesCollector, ZfsCollector,
};
use sanview::domain::TopologyCorrelator;
use sanview::ui::{run_tui, AppState};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "sanview")]
#[command(about = "FreeBSD Storage Array Monitor - real-time TUI for storage systems")]
#[command(version)]
struct Args {
    /// Refresh interval in milliseconds
    #[arg(short, long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(50..=10000))]
    refresh: u64,
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Initialize collectors
    let mut geom_collector = GeomCollector::new()
        .context("Failed to initialize GEOM collector")?;
    let mut multipath_collector = MultipathCollector::new();
    let ses_collector = SesCollector::new();
    let mut zfs_collector = ZfsCollector::new();
    let topology_correlator = TopologyCorrelator::new();

    // Initialize system stats collectors
    let mut cpu_collector = CpuCollector::new();
    let memory_collector = MemoryCollector::new();
    let mut network_collector = NetworkCollector::new();
    let bhyve_collector = BhyveCollector::new();
    let jail_collector = JailCollector::new();

    // Collect SES slot mappings once (static data)
    let ses_info = match ses_collector.collect() {
        Ok(info) => {
            log::info!("Found {} disk slot mappings via SES", info.len());
            info
        }
        Err(e) => {
            log::warn!("Failed to collect SES data: {}", e);
            log::warn!("Continuing without slot mapping...");
            std::collections::HashMap::new()
        }
    };

    // Create shared application state
    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Run TUI in a separate thread (TUI can be Send, but GEOM FFI cannot)
    let tui_state = Arc::clone(&app_state);
    let tui_handle = std::thread::spawn(move || {
        run_tui(tui_state)
    });

    // Run data collection in main thread (required because GEOM FFI is not Send)
    let mut last_update = std::time::Instant::now();
    let mut last_slow_update = std::time::Instant::now();

    loop {
        // Check if TUI thread has finished (user quit)
        if tui_handle.is_finished() {
            break;
        }

        // Fast refresh for storage/CPU/memory stats
        if last_update.elapsed() >= Duration::from_millis(args.refresh) {
            last_update = std::time::Instant::now();

            // Collect raw disk statistics
            let physical_disks = match geom_collector.collect() {
                Ok(disks) => disks,
                Err(e) => {
                    log::error!("Error collecting GEOM statistics: {}", e);
                    continue;
                }
            };

            // Collect multipath topology
            let multipath_info = match multipath_collector.collect() {
                Ok(info) => info,
                Err(e) => {
                    log::error!("Error collecting multipath topology: {}", e);
                    continue;
                }
            };

            // Collect ZFS topology
            let zfs_info = match zfs_collector.collect() {
                Ok(info) => info,
                Err(e) => {
                    log::warn!("Error collecting ZFS topology: {}", e);
                    std::collections::HashMap::new()
                }
            };

            // Correlate and deduplicate
            let (multipath_devices, standalone_disks) =
                topology_correlator.correlate(physical_disks, multipath_info, ses_info.clone(), zfs_info);

            // Collect system stats
            let cpu_stats = cpu_collector.collect().unwrap_or_else(|e| {
                log::error!("Error collecting CPU stats: {}", e);
                sanview::collectors::CpuStats { cores: Vec::new() }
            });

            let memory_stats = memory_collector.collect().unwrap_or_else(|e| {
                log::error!("Error collecting memory stats: {}", e);
                sanview::collectors::MemoryStats {
                    total_bytes: 0,
                    active_bytes: 0,
                    inactive_bytes: 0,
                    laundry_bytes: 0,
                    wired_bytes: 0,
                    buf_bytes: 0,
                    free_bytes: 0,
                    used_pct: 0.0,
                    swap_total_bytes: 0,
                    swap_used_bytes: 0,
                    swap_used_pct: 0.0,
                    arc_total_bytes: 0,
                    arc_mfu_bytes: 0,
                    arc_mru_bytes: 0,
                    arc_anon_bytes: 0,
                    arc_header_bytes: 0,
                    arc_other_bytes: 0,
                    arc_compressed_bytes: 0,
                    arc_uncompressed_bytes: 0,
                    arc_ratio: 0.0,
                }
            });

            let network_stats = network_collector.collect().unwrap_or_else(|e| {
                log::warn!("Error collecting network stats: {}", e);
                Vec::new()
            });

            // Collect VMs and jails less frequently (8x the refresh interval, min 2s)
            let slow_interval = (args.refresh * 8).max(2000);
            let (vms, jails) = if last_slow_update.elapsed() >= Duration::from_millis(slow_interval) {
                last_slow_update = std::time::Instant::now();
                let v = bhyve_collector.collect().unwrap_or_else(|e| {
                    log::warn!("Error collecting bhyve VMs: {}", e);
                    Vec::new()
                });
                let j = jail_collector.collect().unwrap_or_else(|e| {
                    log::warn!("Error collecting jails: {}", e);
                    Vec::new()
                });
                (v, j)
            } else {
                // Use previous values
                let state = app_state.lock().unwrap();
                (state.vms.clone(), state.jails.clone())
            };

            // Update shared state
            {
                let mut state = app_state.lock().unwrap();
                state.update_topology(multipath_devices, standalone_disks);
                state.update_system_stats(cpu_stats, memory_stats, network_stats, vms, jails);
            }
        }

        // Small sleep to avoid busy waiting
        std::thread::sleep(Duration::from_millis(50));
    }

    // Wait for TUI thread to finish
    tui_handle.join().expect("TUI thread panicked")?;

    Ok(())
}
