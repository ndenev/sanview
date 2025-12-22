use crate::collectors::ZfsRole;
use crate::domain::device::MultipathDevice;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Sparkline},
    Frame,
};
use std::collections::{HashMap, VecDeque};

/// Render a front panel view with vertical 2.5" drives and activity LEDs
pub fn render_front_panel(
    frame: &mut Frame,
    area: Rect,
    devices: &[MultipathDevice],
    iops_history: &VecDeque<f64>,
    read_bw_history: &VecDeque<f64>,
    write_bw_history: &VecDeque<f64>,
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
            Constraint::Percentage(55),  // Left: drives visual + cumulative sparklines
            Constraint::Percentage(45),  // Right: per-drive stats (full height)
        ])
        .split(inner);

    // Split left section vertically: drives (top) and cumulative sparklines (bottom)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Drives visual (7) + legend (1)
            Constraint::Min(4),      // Cumulative sparklines (remaining space)
        ])
        .split(horiz_chunks[0]);

    // Layout drives area with legend
    // Drive bay: 2 outer border + 3 content + 2 drive border = 7 lines
    let drive_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),   // Drive bay with outer border
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
    render_storage_sparklines(
        frame,
        left_chunks[1],
        iops_history,
        read_bw_history,
        write_bw_history,
        busy_history,
    );

    // Render per-drive stats panel on right side (full height)
    render_drive_stats(frame, horiz_chunks[1], devices, drive_busy_history);
}

fn render_storage_sparklines(
    frame: &mut Frame,
    area: Rect,
    iops_history: &VecDeque<f64>,
    read_bw_history: &VecDeque<f64>,
    write_bw_history: &VecDeque<f64>,
    busy_history: &VecDeque<f64>,
) {
    // Split into 4 rows for different metrics, adapting to available height
    // Each row needs at least 2 lines (1 label + 1 sparkline)
    let row_height = (area.height / 4).max(2);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(row_height),
            Constraint::Length(row_height),
            Constraint::Length(row_height),
            Constraint::Length(row_height),
        ])
        .split(area);

    // Helper to render a single sparkline with label
    let render_sparkline = |frame: &mut Frame, chunk: Rect, history: &VecDeque<f64>, label: String, color: Color| {
        if history.is_empty() || chunk.height < 2 {
            return;
        }

        let sub_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),       // Label
                Constraint::Min(1),          // Sparkline (remaining height)
            ])
            .split(chunk);

        // Sliding window: take last N points that fit the width
        let width = sub_chunks[1].width as usize;
        let start = if history.len() > width {
            history.len() - width
        } else {
            0
        };
        let data: Vec<u64> = history.iter().skip(start).map(|&v| v as u64).collect();

        let paragraph = Paragraph::new(label)
            .style(Style::default().fg(color));
        frame.render_widget(paragraph, sub_chunks[0]);

        let sparkline = Sparkline::default()
            .data(&data)
            .style(Style::default().fg(color))
            .bar_set(ratatui::symbols::bar::NINE_LEVELS);
        frame.render_widget(sparkline, sub_chunks[1]);
    };

    // IOPS sparkline
    let max_iops = iops_history.iter().cloned().fold(0.0f64, f64::max);
    let iops_label = format!("IOPS: {:.0} (max: {:.0})", iops_history.back().unwrap_or(&0.0), max_iops);
    render_sparkline(frame, chunks[0], iops_history, iops_label, Color::Green);

    // Read BW sparkline
    let max_read = read_bw_history.iter().cloned().fold(0.0f64, f64::max);
    let read_label = format!("Read: {:.1} MB/s (max: {:.1})", read_bw_history.back().unwrap_or(&0.0), max_read);
    render_sparkline(frame, chunks[1], read_bw_history, read_label, Color::Cyan);

    // Write BW sparkline
    let max_write = write_bw_history.iter().cloned().fold(0.0f64, f64::max);
    let write_label = format!("Write: {:.1} MB/s (max: {:.1})", write_bw_history.back().unwrap_or(&0.0), max_write);
    render_sparkline(frame, chunks[2], write_bw_history, write_label, Color::Yellow);

    // Busy % sparkline
    let max_busy = busy_history.iter().cloned().fold(0.0f64, f64::max);
    let busy_label = format!("Busy: {:.1}% (max: {:.1}%)", busy_history.back().unwrap_or(&0.0), max_busy);
    render_sparkline(frame, chunks[3], busy_history, busy_label, Color::Gray);
}

fn render_drive_stats(
    frame: &mut Frame,
    area: Rect,
    devices: &[MultipathDevice],
    drive_busy_history: &HashMap<String, VecDeque<f64>>,
) {
    let block = Block::default()
        .title(format!(" Drives ({}) ", devices.len()))
        .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM)
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

    let available_height = inner.height as usize;
    let drives_to_show = available_height.min(slot_devices.len());

    for (idx, (slot, dev)) in slot_devices.iter().take(drives_to_show).enumerate() {
        let y_pos = inner.y + idx as u16;
        if y_pos >= inner.y + inner.height {
            break;
        }

        let line_area = Rect {
            x: inner.x,
            y: y_pos,
            width: inner.width,
            height: 1,
        };

        // Use actual physical slot number (already 1-based from SES element number)
        let slot_label = format!("{:02}", slot);

        // Extract device serial from multipath name
        let serial = dev.name.split('/').last().unwrap_or(&dev.name);

        // Get ZFS vdev info - format: role(vdev_index) e.g., "data(0)", "log", "cache", "spare"
        let vdev_label = if let Some(ref zfs_info) = dev.zfs_info {
            // Extract vdev index number (e.g., "raidz1-0" -> "0", "mirror-2" -> "2")
            let vdev_idx = zfs_info.vdev
                .split('-')
                .last()
                .and_then(|s| s.parse::<u32>().ok());

            match zfs_info.role {
                ZfsRole::Data => {
                    if let Some(idx) = vdev_idx {
                        format!("data({})", idx)
                    } else {
                        "data".to_string()
                    }
                }
                ZfsRole::Slog => {
                    if let Some(idx) = vdev_idx {
                        format!("log({})", idx)
                    } else {
                        "log".to_string()
                    }
                }
                ZfsRole::Cache => {
                    if let Some(idx) = vdev_idx {
                        format!("cache({})", idx)
                    } else {
                        "cache".to_string()
                    }
                }
                ZfsRole::Spare => "spare".to_string(),
            }
        } else {
            "".to_string()
        };

        // Determine colors based on ZFS role and busy %
        let busy_color = if dev.statistics.busy_pct > 80.0 {
            Color::Red
        } else if dev.statistics.busy_pct > 50.0 {
            Color::Yellow
        } else if dev.statistics.busy_pct > 0.1 {
            Color::Green
        } else {
            Color::DarkGray
        };

        let role_color = if let Some(ref zfs_info) = dev.zfs_info {
            match zfs_info.role {
                ZfsRole::Slog => Color::Yellow,
                ZfsRole::Cache => Color::Magenta,
                ZfsRole::Spare => Color::Blue,
                ZfsRole::Data => Color::Cyan,
            }
        } else {
            Color::DarkGray
        };

        // Fixed-width columns for alignment:
        // slot: 2 chars, vdev: 8 chars, serial: 8 chars, busy: 5 chars, spaces: 4
        // Total fixed prefix: 27 chars
        const SLOT_WIDTH: usize = 2;
        const VDEV_WIDTH: usize = 8;
        const SERIAL_WIDTH: usize = 8;
        const BUSY_WIDTH: usize = 5;
        const FIXED_PREFIX: u16 = (SLOT_WIDTH + 1 + VDEV_WIDTH + 1 + SERIAL_WIDTH + 1 + BUSY_WIDTH + 1) as u16;

        // Pad/truncate vdev label to fixed width
        let vdev_padded = format!("{:<width$}", vdev_label, width = VDEV_WIDTH);

        // Pad/truncate serial to fixed width
        let serial_padded = format!("{:<width$}", serial, width = SERIAL_WIDTH);

        // Format busy % with fixed width
        let busy_text = format!("{:>4.0}%", dev.statistics.busy_pct);

        // Calculate sparkline width (remaining space)
        let sparkline_width = if inner.width > FIXED_PREFIX {
            (inner.width - FIXED_PREFIX) as usize
        } else {
            0
        };

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

            // Render text: slot, vdev role, serial, busy %
            let spans = vec![
                Span::styled(&slot_label, Style::default().fg(Color::White)),
                Span::raw(" "),
                Span::styled(&vdev_padded, Style::default().fg(role_color)),
                Span::raw(" "),
                Span::styled(&serial_padded, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(&busy_text, Style::default().fg(busy_color)),
                Span::raw(" "),
            ];

            let text = Line::from(spans);
            let text_widget = Paragraph::new(text);
            frame.render_widget(text_widget, text_area);

            // Render sparkline if we have history for this device
            if let Some(history) = drive_busy_history.get(&dev.name) {
                if !history.is_empty() {
                    // Sliding window: take last N points that fit the width
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
            // Not enough space for sparkline, just show text
            let spans = vec![
                Span::styled(&slot_label, Style::default().fg(Color::White)),
                Span::raw(" "),
                Span::styled(&vdev_padded, Style::default().fg(role_color)),
                Span::raw(" "),
                Span::styled(&serial_padded, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(&busy_text, Style::default().fg(busy_color)),
            ];

            let text = Line::from(spans);
            let text_widget = Paragraph::new(text);
            frame.render_widget(text_widget, line_area);
        }
    }
}

fn render_vertical_drive(frame: &mut Frame, area: Rect, slot: usize, devices: &[MultipathDevice]) {
    // Find device for this slot
    // For now, we'll use a simple mapping - in the future this will use proper SES mapping
    let device_info = find_device_for_slot(slot, devices);

    // Slot number as vertical digits (1-based)
    let slot_num = slot + 1;
    let digit1 = format!("{}", slot_num / 10); // tens digit (0 for slots 1-9)
    let digit2 = format!("{}", slot_num % 10); // ones digit

    let (drive_visual, border_color) = match device_info {
        Some((_dev, stats)) => {
            // Determine blink state based on current time and activity
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            let blink = (now.as_millis() / 250) % 2 == 0; // Toggle every 250ms

            // Activity indicator - colored circles
            let has_read = stats.read_iops > 0.1;
            let has_write = stats.write_iops > 0.1;
            let (activity_color, activity_char) = match (has_read, has_write) {
                (true, true) => (Color::Magenta, if blink { "●" } else { "○" }),
                (true, false) => (Color::Green, if blink { "●" } else { "○" }),
                (false, true) => (Color::Yellow, if blink { "●" } else { "○" }),
                (false, false) => (Color::DarkGray, "○"),
            };

            // Build vertical drive visualization: slot number on top, activity at bottom
            let visual = vec![
                Line::from(Span::styled(&digit1, Style::default().fg(Color::White))),
                Line::from(Span::styled(&digit2, Style::default().fg(Color::White))),
                Line::from(Span::styled(activity_char, Style::default().fg(activity_color))),
            ];

            // Color code border by busy percentage
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
            // Empty slot - show slot number vertically
            let visual = vec![
                Line::from(Span::styled(&digit1, Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled(&digit2, Style::default().fg(Color::DarkGray))),
                Line::from(" "),
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
) -> Option<(&MultipathDevice, &crate::domain::device::DiskStatistics)> {
    // UI slot is 0-based (0-24), SES slot is 1-based (1-25)
    // Find device where device.slot matches the physical slot number
    let physical_slot = slot + 1;
    devices
        .iter()
        .find(|dev| dev.slot == Some(physical_slot))
        .map(|dev| (dev, &dev.statistics))
}
