# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SANVIEW is a FreeBSD TUI application for real-time monitoring of storage arrays. It displays storage hardware status, system performance metrics, and virtualization information with a visual front panel representation of physical drive slots.

## Build Commands

```bash
cargo build --release    # Build release binary
sudo ./target/release/sanview  # Run with required privileges
sudo ./target/release/sanview -r 100  # Custom refresh interval (ms)
```

## CLI Options

- `-r, --refresh <ms>` - Refresh interval in milliseconds (default: 250, range: 50-10000)
- `-h, --help` - Show help
- `-V, --version` - Show version

VMs/jails are polled at 8x the refresh interval (minimum 2 seconds).

## Architecture

### Threading Model

The application uses a **dual-thread architecture** to work around FreeBSD libgeom FFI limitations (not Send/Sync):

- **Main Thread**: Runs all data collectors (GEOM requires this thread)
- **UI Thread**: Renders TUI via ratatui, shares state via `Arc<Mutex<AppState>>`

### Data Flow

```
Collectors (main thread)            State (shared)          UI (100ms polling)
├─ GeomCollector ─────────────┐
├─ MultipathCollector ────────┼─> TopologyCorrelator ─> AppState ─> Ratatui render
├─ SesCollector ──────────────┤     (correlates &        (dynamic history
├─ ZfsCollector ──────────────┘      deduplicates)         based on terminal width)
├─ CpuCollector ──────────────────────────────────────>
└─ MemoryCollector ───────────────────────────────────>

Slow collectors (8x refresh): BhyveCollector, JailCollector
```

### Module Structure

- **collectors/** - Nine FreeBSD-specific data collectors:
  - `geom.rs` - Disk I/O stats via libgeom FFI (IOPS, bandwidth, latency, busy%)
  - `multipath.rs` - Parses `kern.geom.conftxt` for multipath topology
  - `ses.rs` - SCSI Enclosure Services ioctls for physical slot mapping
  - `zfs.rs` - Parses `zpool status` for pool/vdev/role info
  - `cpu.rs`, `memory.rs` - System stats via sysctl
  - `bhyve.rs`, `jail.rs` - VM/container enumeration

- **domain/** - Data models and correlation logic:
  - `device.rs` - `PhysicalDisk`, `MultipathDevice`, `DiskStatistics` types
  - `topology.rs` - `TopologyCorrelator` combines collector data, groups disks under multipath devices, deduplicates paths

- **ui/** - Ratatui TUI components:
  - `app.rs` - Main event loop, layout, keyboard handling, terminal width tracking
  - `state.rs` - `AppState` with current metrics + dynamic history buffers (sized to terminal width)
  - `components/front_panel.rs` - Layout: left side has 25-slot drive visual + cumulative sparklines; right side has full-height per-drive stats panel
  - `components/system_overview.rs` - CPU gauges, memory, VMs, jails
  - `components/stats_table.rs` - Tabular storage statistics

### Key Design Patterns

1. **Stateful collectors**: GEOM and CPU collectors maintain previous snapshots for delta-based rate calculations
2. **Correlation/enrichment**: TopologyCorrelator joins data from multiple sources into unified device view
3. **Deduplication**: Multiple paths to same physical disk are grouped, not double-counted
4. **Graceful degradation**: Collectors fail silently; app continues with available data
5. **Dynamic history sizing**: Sparkline history buffers resize based on terminal width (min 60 entries)

## FreeBSD-Specific Notes

- Requires root privileges for GEOM statistics and SES ioctls
- Multipath device names follow `multipath/SERIAL` convention
- ZFS ARC metrics read from `kstat.zfs.misc.arcstats.*` sysctl
- CPU times from `kern.cp_times` sysctl (per-core)

## Recent Changes (Session Context)

Layout was restructured in `front_panel.rs`:
- Left side (55%): drives visual (top) + cumulative sparklines (bottom)
- Right side (45%): per-drive stats panel extends full height
- Sparklines adapt to variable height dynamically

History sizing changed in `state.rs`:
- No longer hardcoded; tracks terminal width via `set_terminal_width()`
- `app.rs` updates width on each frame from `terminal.size()`
