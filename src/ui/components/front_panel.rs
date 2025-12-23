use crate::collectors::ZfsRole;
use crate::domain::device::MultipathDevice;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph, Sparkline},
    Frame,
};
use std::collections::{HashMap, VecDeque};

/// Render a front panel view with vertical 2.5" drives and activity LEDs
pub fn render_front_panel(
    frame: &mut Frame,
    area: Rect,
    devices: &[MultipathDevice],
    read_iops_history: &VecDeque<f64>,
    write_iops_history: &VecDeque<f64>,
    read_bw_history: &VecDeque<f64>,
    write_bw_history: &VecDeque<f64>,
    read_latency_history: &VecDeque<f64>,
    write_latency_history: &VecDeque<f64>,
    queue_depth_history: &VecDeque<f64>,
    busy_history: &VecDeque<f64>,
    drive_busy_history: &HashMap<String, VecDeque<f64>>,
) {
    let block = Block::default()
        .title(" Storage Array - EMC2 25-Bay (Vertical 2.5\" SAS) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split horizontally: left (drives + sparklines) and right (per-drive stats full height)
    let horiz_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(65),  // Left: drives visual + cumulative sparklines
            Constraint::Percentage(35),  // Right: per-drive stats (narrower)
        ])
        .split(inner);

    // Split left section vertically: drives (top) and cumulative sparklines (bottom)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),   // Drives visual (8) + legend (1)
            Constraint::Fill(1),     // Cumulative sparklines (fills all remaining space)
        ])
        .split(horiz_chunks[0]);

    // Layout drives area with legend
    // Drive bay: 2 outer border + 4 content + 2 drive border = 8 lines
    let drive_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Drive bay with outer border
            Constraint::Length(1),   // Legend
        ])
        .split(left_chunks[0]);

    let drive_area = drive_chunks[0];

    // Create drive bay with border: 25 drives
    // Each slot is 3 chars wide, total = 75 chars + 2 for outer border = 77 chars
    let total_bay_width: u16 = 25 * 3 + 2; // 25 slots * 3 chars + 2 border chars

    // Center the drive bay in the available area
    let left_padding = if drive_area.width > total_bay_width {
        (drive_area.width - total_bay_width) / 2
    } else {
        0
    };

    let centered_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_padding),
            Constraint::Length(total_bay_width.min(drive_area.width)),
            Constraint::Min(0),
        ])
        .split(drive_area);

    // Draw outer border around the drive bay
    let bay_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let bay_inner = bay_block.inner(centered_chunks[1]);
    frame.render_widget(bay_block, centered_chunks[1]);

    // Create 25 columns for drives
    let constraints: Vec<Constraint> = (0..25)
        .map(|_| Constraint::Length(3))
        .collect();

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(bay_inner);

    for (slot, col_area) in cols.iter().enumerate() {
        render_vertical_drive(frame, *col_area, slot, devices);
    }

    // Render legend
    let legend = Paragraph::new(Line::from(vec![
        Span::styled("●", Style::default().fg(Color::Green)),
        Span::raw(" Rd "),
        Span::styled("●", Style::default().fg(Color::Yellow)),
        Span::raw(" Wr "),
        Span::styled("●", Style::default().fg(Color::Magenta)),
        Span::raw(" R+W "),
        Span::styled("○", Style::default().fg(Color::DarkGray)),
        Span::raw(" Idle"),
    ]));

    frame.render_widget(legend, drive_chunks[1]);

    // Render cumulative sparklines below drives
    render_storage_charts(
        frame,
        left_chunks[1],
        read_iops_history,
        write_iops_history,
        read_bw_history,
        write_bw_history,
        read_latency_history,
        write_latency_history,
        queue_depth_history,
        busy_history,
    );

    // Render per-drive stats panel on right side (full height)
    render_drive_stats(frame, horiz_chunks[1], devices, drive_busy_history);
}

fn render_storage_charts(
    frame: &mut Frame,
    area: Rect,
    read_iops_history: &VecDeque<f64>,
    write_iops_history: &VecDeque<f64>,
    read_bw_history: &VecDeque<f64>,
    write_bw_history: &VecDeque<f64>,
    read_latency_history: &VecDeque<f64>,
    write_latency_history: &VecDeque<f64>,
    queue_depth_history: &VecDeque<f64>,
    _busy_history: &VecDeque<f64>,
) {
    // Split into 4 equal rows for different metrics
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ])
        .split(area);

    // Helper to render a chart with label on separate line above
    let render_chart = |frame: &mut Frame,
                        chunk: Rect,
                        history: &VecDeque<f64>,
                        label: String,
                        color: Color| {
        if chunk.height < 2 {
            return;
        }

        // Split: 1 line for label, rest for chart
        let sub_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Label line (fixed)
                Constraint::Fill(1),    // Chart (fills remaining)
            ])
            .split(chunk);

        // Render label
        let label_widget = Paragraph::new(label)
            .style(Style::default().fg(Color::White));
        frame.render_widget(label_widget, sub_chunks[0]);

        // Render chart if we have space
        if sub_chunks[1].height < 1 || history.is_empty() {
            return;
        }

        // Use chart width to determine how many points to display
        // Each braille character is 2 dots wide, so we can fit width * 2 points
        let chart_width = sub_chunks[1].width as usize;
        let max_points = chart_width * 2;

        // Take the most recent points (history is pre-filled so always has enough)
        let start = history.len().saturating_sub(max_points);
        let data: Vec<(f64, f64)> = history
            .iter()
            .skip(start)
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect();

        // Find max Y value for scaling
        let max_y = history.iter().cloned().fold(1.0_f64, f64::max) * 1.1;

        let dataset = Dataset::default()
            .marker(Marker::Braille)
            .graph_type(ratatui::widgets::GraphType::Line)
            .style(Style::default().fg(color))
            .data(&data);

        // X bounds match actual data length
        let x_max = (data.len().saturating_sub(1)) as f64;
        let chart = Chart::new(vec![dataset])
            .x_axis(
                Axis::default()
                    .bounds([0.0, x_max.max(1.0)])
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, max_y.max(1.0)])
            )
            .hidden_legend_constraints((Constraint::Ratio(0, 1), Constraint::Ratio(0, 1)));

        frame.render_widget(chart, sub_chunks[1]);
    };

    // Helper to combine two histories into total
    let combine_histories = |h1: &VecDeque<f64>, h2: &VecDeque<f64>| -> VecDeque<f64> {
        let len = h1.len().max(h2.len());
        let mut combined = VecDeque::with_capacity(len);
        for i in 0..len {
            let v1 = h1.get(i).unwrap_or(&0.0);
            let v2 = h2.get(i).unwrap_or(&0.0);
            combined.push_back(v1 + v2);
        }
        combined
    };

    // IOPS (combined read + write)
    let total_iops = combine_histories(read_iops_history, write_iops_history);
    let cur_read_iops = read_iops_history.back().unwrap_or(&0.0);
    let cur_write_iops = write_iops_history.back().unwrap_or(&0.0);
    let iops_label = format!("IOPS: R:{:.0} W:{:.0} T:{:.0}", cur_read_iops, cur_write_iops, cur_read_iops + cur_write_iops);
    render_chart(frame, chunks[0], &total_iops, iops_label, Color::Cyan);

    // Throughput (combined read + write)
    let total_bw = combine_histories(read_bw_history, write_bw_history);
    let cur_read_bw = read_bw_history.back().unwrap_or(&0.0);
    let cur_write_bw = write_bw_history.back().unwrap_or(&0.0);
    let bw_label = format!("MB/s: R:{:.1} W:{:.1} T:{:.1}", cur_read_bw, cur_write_bw, cur_read_bw + cur_write_bw);
    render_chart(frame, chunks[1], &total_bw, bw_label, Color::Green);

    // Latency (show max of read/write for worst-case view)
    let max_latency: VecDeque<f64> = read_latency_history.iter()
        .zip(write_latency_history.iter())
        .map(|(r, w)| r.max(*w))
        .collect();
    let cur_read_lat = read_latency_history.back().unwrap_or(&0.0);
    let cur_write_lat = write_latency_history.back().unwrap_or(&0.0);
    let lat_label = format!("Latency(ms): R:{:.1} W:{:.1}", cur_read_lat, cur_write_lat);
    render_chart(frame, chunks[2], &max_latency, lat_label, Color::Yellow);

    // Queue depth
    let cur_qd = queue_depth_history.back().unwrap_or(&0.0);
    let qd_label = format!("Queue Depth: {:.0}", cur_qd);
    render_chart(frame, chunks[3], queue_depth_history, qd_label, Color::Magenta);
}

fn render_drive_stats(
    frame: &mut Frame,
    area: Rect,
    devices: &[MultipathDevice],
    drive_busy_history: &HashMap<String, VecDeque<f64>>,
) {
    // Just use left border as separator (main panel provides outer border)
    let block = Block::default()
        .title(format!(" Drives ({}) ", devices.len()))
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if devices.is_empty() {
        let placeholder = Paragraph::new("No drives detected")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Sort devices by physical SES slot (if available), otherwise by name
    let mut sorted_devices: Vec<&MultipathDevice> = devices.iter().collect();
    sorted_devices.sort_by(|a, b| {
        match (a.slot, b.slot) {
            (Some(slot_a), Some(slot_b)) => slot_a.cmp(&slot_b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        }
    });

    // Create display list with physical slot numbers
    let slot_devices: Vec<(usize, &MultipathDevice)> = sorted_devices
        .iter()
        .map(|&dev| {
            let display_slot = dev.slot.unwrap_or(0);
            (display_slot, dev)
        })
        .collect();

    // Column widths - expanded layout with more ZFS info
    // SL POOL ROLE  VDEV S  IOPS MB/s BSY [sparkline]
    const SLOT_W: usize = 2;
    const POOL_W: usize = 4;
    const ROLE_W: usize = 5;
    const VDEV_W: usize = 4;
    const STATE_W: usize = 1;
    const IOPS_W: usize = 5;
    const BW_W: usize = 5;
    const BUSY_W: usize = 3;
    // Total: 2+1+4+1+5+1+4+1+1+1+5+1+5+1+3+1 = 37 chars before sparkline
    const FIXED_PREFIX: u16 = (SLOT_W + 1 + POOL_W + 1 + ROLE_W + 1 + VDEV_W + 1 + STATE_W + 1 + IOPS_W + 1 + BW_W + 1 + BUSY_W + 1) as u16;

    // Render header if we have space
    let available_height = inner.height as usize;
    let show_header = available_height > 1;
    let header_offset: u16 = if show_header { 1 } else { 0 };

    if show_header {
        let header_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        let header = Line::from(vec![
            Span::styled(format!("{:<SLOT_W$}", "SL"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:<POOL_W$}", "POOL"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:<ROLE_W$}", "ROLE"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:<VDEV_W$}", "VDEV"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled("S", Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:>IOPS_W$}", "IOPS"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:>BW_W$}", "MB/s"), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:>BUSY_W$}", "BSY"), Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(header), header_area);
    }

    let drives_to_show = (available_height - header_offset as usize).min(slot_devices.len());

    for (idx, (slot, dev)) in slot_devices.iter().take(drives_to_show).enumerate() {
        let y_pos = inner.y + header_offset + idx as u16;
        if y_pos >= inner.y + inner.height {
            break;
        }

        let line_area = Rect {
            x: inner.x,
            y: y_pos,
            width: inner.width,
            height: 1,
        };

        // Slot number
        let slot_label = format!("{:02}", slot);

        // Pool name (truncated)
        let pool_name = dev.zfs_info.as_ref()
            .map(|z| truncate_str(&z.pool, POOL_W))
            .unwrap_or_else(|| "-".to_string());

        // Role name and color
        let (role_name, role_color) = if let Some(ref zfs_info) = dev.zfs_info {
            match zfs_info.role {
                ZfsRole::Data => ("data", Color::Cyan),
                ZfsRole::Slog => ("log", Color::Yellow),
                ZfsRole::Cache => ("cache", Color::Magenta),
                ZfsRole::Spare => ("spare", Color::Blue),
            }
        } else {
            ("-", Color::DarkGray)
        };

        // Vdev topology shorthand: raidz1-0 -> r1-0, mirror-5 -> mi-5
        // Shows "-" for devices without a vdev (individual cache/spare)
        let vdev_short = if let Some(ref zfs_info) = dev.zfs_info {
            let vdev = &zfs_info.vdev;
            if vdev.starts_with("raidz3") {
                vdev.replace("raidz3-", "r3-")
            } else if vdev.starts_with("raidz2") {
                vdev.replace("raidz2-", "r2-")
            } else if vdev.starts_with("raidz1") {
                vdev.replace("raidz1-", "r1-")
            } else if vdev.starts_with("raidz") {
                vdev.replace("raidz-", "rz-")
            } else if vdev.starts_with("mirror") {
                vdev.replace("mirror-", "mi-")
            } else if vdev.is_empty() {
                "-".to_string()
            } else {
                truncate_str(vdev, VDEV_W)
            }
        } else {
            "-".to_string()
        };
        let vdev_padded = format!("{:<VDEV_W$}", truncate_str(&vdev_short, VDEV_W));

        // State indicator (colored dot)
        let (state_char, state_color) = if let Some(ref zfs_info) = dev.zfs_info {
            match zfs_info.state.to_uppercase().as_str() {
                "ONLINE" => ("●", Color::Green),
                "DEGRADED" => ("●", Color::Yellow),
                "FAULTED" | "UNAVAIL" | "OFFLINE" => ("●", Color::Red),
                "AVAIL" => ("○", Color::Green),  // Spare available
                _ => ("○", Color::DarkGray),
            }
        } else {
            ("○", Color::DarkGray)
        };

        // IOPS (total read + write)
        let total_iops = dev.statistics.total_iops();
        let iops_text = if total_iops >= 10000.0 {
            format!("{:>4.0}k", total_iops / 1000.0)
        } else {
            format!("{:>IOPS_W$.0}", total_iops)
        };

        // Throughput MB/s (total)
        let total_bw = dev.statistics.total_bw_mbps();
        let bw_text = if total_bw >= 1000.0 {
            format!("{:>4.1}G", total_bw / 1000.0)
        } else {
            format!("{:>BW_W$.1}", total_bw)
        };

        // Busy %
        let busy_pct = dev.statistics.busy_pct;
        let busy_text = format!("{:>2.0}%", busy_pct.min(99.0));
        let busy_color = if busy_pct > 80.0 {
            Color::Red
        } else if busy_pct > 50.0 {
            Color::Yellow
        } else if busy_pct > 0.1 {
            Color::Green
        } else {
            Color::DarkGray
        };

        // Calculate sparkline width (remaining space)
        let sparkline_width = if inner.width > FIXED_PREFIX {
            (inner.width - FIXED_PREFIX) as usize
        } else {
            0
        };

        // Build spans
        let mut spans = vec![
            Span::styled(&slot_label, Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(format!("{:<POOL_W$}", pool_name), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{:<ROLE_W$}", role_name), Style::default().fg(role_color)),
            Span::raw(" "),
            Span::styled(&vdev_padded, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(state_char, Style::default().fg(state_color)),
            Span::raw(" "),
            Span::styled(&iops_text, Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(&bw_text, Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(&busy_text, Style::default().fg(busy_color)),
            Span::raw(" "),
        ];

        if sparkline_width > 0 {
            // Split area: text on left, sparkline on right
            let text_area = Rect {
                x: line_area.x,
                y: line_area.y,
                width: FIXED_PREFIX,
                height: 1,
            };

            let sparkline_area = Rect {
                x: line_area.x + FIXED_PREFIX,
                y: line_area.y,
                width: sparkline_width as u16,
                height: 1,
            };

            let text = Line::from(spans);
            frame.render_widget(Paragraph::new(text), text_area);

            // Render sparkline if we have history for this device
            if let Some(history) = drive_busy_history.get(&dev.name) {
                if !history.is_empty() {
                    let start = if history.len() > sparkline_width {
                        history.len() - sparkline_width
                    } else {
                        0
                    };
                    let data: Vec<u64> = history.iter().skip(start).map(|&v| v as u64).collect();
                    let sparkline = Sparkline::default()
                        .data(&data)
                        .style(Style::default().fg(Color::Cyan))
                        .bar_set(ratatui::symbols::bar::NINE_LEVELS);
                    frame.render_widget(sparkline, sparkline_area);
                }
            }
        } else {
            // Not enough space for sparkline, just show text without trailing space
            spans.pop(); // Remove trailing space
            let text = Line::from(spans);
            frame.render_widget(Paragraph::new(text), line_area);
        }
    }
}

/// Truncate a string to max_len characters
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        s[..max_len].to_string()
    }
}

fn render_vertical_drive(frame: &mut Frame, area: Rect, slot: usize, devices: &[MultipathDevice]) {
    // Find device for this slot
    let device = find_device_for_slot(slot, devices);

    // Slot number as vertical digits (1-based)
    let slot_num = slot + 1;
    let digit1 = format!("{}", slot_num / 10); // tens digit (0 for slots 1-9)
    let digit2 = format!("{}", slot_num % 10); // ones digit

    let (drive_visual, border_color) = match device {
        Some(dev) => {
            // Determine blink state based on current time and activity
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            let blink = (now.as_millis() / 250) % 2 == 0; // Toggle every 250ms

            // Get per-controller activity from path_stats
            // Controller A (0) LED at top, Controller B (1) LED at bottom
            let ctrl_a_stats = dev.path_stats.iter().find(|p| p.controller == 0);
            let ctrl_b_stats = dev.path_stats.iter().find(|p| p.controller == 1);

            // Helper to determine LED state for a controller's path
            // Passive paths show crossed circle, active paths show activity-based LED
            let get_led = |path_stats: Option<&crate::domain::device::PathStats>| -> (Color, &str) {
                match path_stats {
                    Some(ps) => {
                        if !ps.is_active {
                            // Passive/standby path - show crossed circle in dark gray
                            (Color::DarkGray, "⊘")
                        } else {
                            // Active path - show activity-based LED
                            let has_read = ps.statistics.read_iops > 0.1;
                            let has_write = ps.statistics.write_iops > 0.1;
                            match (has_read, has_write) {
                                (true, true) => (Color::Magenta, if blink { "●" } else { "○" }),
                                (true, false) => (Color::Green, if blink { "●" } else { "○" }),
                                (false, true) => (Color::Yellow, if blink { "●" } else { "○" }),
                                (false, false) => (Color::DarkGray, "○"),
                            }
                        }
                    }
                    None => (Color::DarkGray, "○"),
                }
            };

            let (led_a_color, led_a_char) = get_led(ctrl_a_stats);
            let (led_b_color, led_b_char) = get_led(ctrl_b_stats);

            // Build vertical drive visualization:
            // Top LED (Controller A), slot digits, Bottom LED (Controller B)
            let visual = vec![
                Line::from(Span::styled(led_a_char, Style::default().fg(led_a_color))),
                Line::from(Span::styled(&digit1, Style::default().fg(Color::White))),
                Line::from(Span::styled(&digit2, Style::default().fg(Color::White))),
                Line::from(Span::styled(led_b_char, Style::default().fg(led_b_color))),
            ];

            // Color code border by busy percentage (from multipath device stats)
            let stats = &dev.statistics;
            let color = if stats.busy_pct > 80.0 {
                Color::Red
            } else if stats.busy_pct > 50.0 {
                Color::Yellow
            } else if stats.total_iops() > 0.1 {
                Color::Green
            } else {
                Color::DarkGray
            };

            (visual, color)
        }
        None => {
            // Empty slot - show slot number vertically with empty LED positions
            let visual = vec![
                Line::from(Span::styled(" ", Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled(&digit1, Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled(&digit2, Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled(" ", Style::default().fg(Color::DarkGray))),
            ];
            (visual, Color::DarkGray)
        }
    };

    let paragraph = Paragraph::new(drive_visual).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(paragraph, area);
}

fn find_device_for_slot(
    slot: usize,
    devices: &[MultipathDevice],
) -> Option<&MultipathDevice> {
    // UI slot is 0-based (0-24), SES slot is 1-based (1-25)
    // Find device where device.slot matches the physical slot number
    let physical_slot = slot + 1;
    devices
        .iter()
        .find(|dev| dev.slot == Some(physical_slot))
}
