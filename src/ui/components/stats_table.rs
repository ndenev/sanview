use crate::domain::device::{MultipathDevice, PhysicalDisk};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

pub fn render_stats_table(
    frame: &mut Frame,
    area: Rect,
    multipath_devices: &[MultipathDevice],
    standalone_disks: &[PhysicalDisk],
) {
    let block = Block::default()
        .title(" Disk Statistics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let header = Row::new(vec![
        Cell::from("Device").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Paths").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Slot").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("R IOPS").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("W IOPS").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Read MB/s").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Write MB/s").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Busy%").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Cell::from("Active Path").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]);

    let mut rows = Vec::new();

    // Add multipath devices
    for mp in multipath_devices {
        let stats = &mp.statistics;

        // Only show devices with activity
        if stats.total_iops() > 0.1 || stats.busy_pct > 0.1 {
            let busy_color = if stats.busy_pct > 80.0 {
                Color::Red
            } else if stats.busy_pct > 50.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            rows.push(Row::new(vec![
                Cell::from(mp.name.clone()),
                Cell::from(format!("{}", mp.paths.len())),
                Cell::from("N/A"),  // TODO: Add slot mapping
                Cell::from(format!("{:.1}", stats.read_iops)),
                Cell::from(format!("{:.1}", stats.write_iops)),
                Cell::from(format!("{:.2}", stats.read_bw_mbps)),
                Cell::from(format!("{:.2}", stats.write_bw_mbps)),
                Cell::from(format!("{:.1}", stats.busy_pct)).style(Style::default().fg(busy_color)),
                Cell::from(mp.active_path.as_deref().unwrap_or("N/A")),
            ]));
        }
    }

    // Add standalone disks if any have activity
    for disk in standalone_disks {
        let stats = &disk.statistics;
        if stats.total_iops() > 0.1 || stats.busy_pct > 0.1 {
            let busy_color = if stats.busy_pct > 80.0 {
                Color::Red
            } else if stats.busy_pct > 50.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            rows.push(Row::new(vec![
                Cell::from(disk.device_name.clone()),
                Cell::from("-"),
                Cell::from(disk.slot.map(|s| format!("{}", s)).unwrap_or_else(|| "N/A".to_string())),
                Cell::from(format!("{:.1}", stats.read_iops)),
                Cell::from(format!("{:.1}", stats.write_iops)),
                Cell::from(format!("{:.2}", stats.read_bw_mbps)),
                Cell::from(format!("{:.2}", stats.write_bw_mbps)),
                Cell::from(format!("{:.1}", stats.busy_pct)).style(Style::default().fg(busy_color)),
                Cell::from("-"),
            ]));
        }
    }

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(25),  // Device
            Constraint::Length(6),   // Paths
            Constraint::Length(5),   // Slot
            Constraint::Length(8),   // R IOPS
            Constraint::Length(8),   // W IOPS
            Constraint::Length(10),  // Read MB/s
            Constraint::Length(10),  // Write MB/s
            Constraint::Length(6),   // Busy%
            Constraint::Length(20),  // Active Path
        ],
    )
    .header(header)
    .block(block)
    .column_spacing(1);

    frame.render_widget(table, area);
}
