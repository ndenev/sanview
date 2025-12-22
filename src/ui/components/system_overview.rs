use crate::collectors::{CpuStats, JailInfo, MemoryStats, NetworkStats, VmInfo};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, List, ListItem, Paragraph, Sparkline},
    Frame,
};
use std::collections::VecDeque;

pub fn render_system_overview(
    frame: &mut Frame,
    area: Rect,
    cpu_stats: &CpuStats,
    memory_stats: &MemoryStats,
    network_stats: &[NetworkStats],
    vms: &[VmInfo],
    jails: &[JailInfo],
    cpu_history: &[VecDeque<f64>],
    memory_history: &VecDeque<f64>,
    _arc_size_history: &VecDeque<f64>,
    _arc_ratio_history: &VecDeque<f64>,
    network_history: &std::collections::HashMap<String, VecDeque<f64>>,
) {
    // Split into left and right sections
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),  // Left: CPU + Memory + Network
            Constraint::Percentage(40),  // Right: VMs + Jails
        ])
        .split(area);

    // Calculate CPU rows needed (each row is 1 line)
    let cores_per_row = 4;
    let cpu_rows = if cpu_stats.cores.is_empty() {
        1
    } else {
        (cpu_stats.cores.len() + cores_per_row - 1) / cores_per_row
    };
    let cpu_height = (cpu_rows as u16) + 2; // +2 for border

    // Memory needs ~4 lines (gauge + sparkline + swap + border)
    let memory_height = 5u16;

    // Network: 1 line per interface + 2 for border, max ~6 interfaces shown
    let net_count = network_stats.len().min(6);
    let network_height = (net_count as u16).max(1) + 2;

    // Left section: CPU, Memory, Network (sized to content)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(cpu_height),
            Constraint::Length(memory_height),
            Constraint::Length(network_height),
            Constraint::Min(0),  // Absorb remaining space
        ])
        .split(main_chunks[0]);

    render_cpu_stats(frame, left_chunks[0], cpu_stats, cpu_history);
    render_memory_stats(frame, left_chunks[1], memory_stats, memory_history);
    render_network_stats(frame, left_chunks[2], network_stats, network_history);

    // Right section: VMs and Jails
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),  // VMs
            Constraint::Percentage(50),  // Jails
        ])
        .split(main_chunks[1]);

    render_vm_list(frame, right_chunks[0], vms);
    render_jail_list(frame, right_chunks[1], jails);
}

fn render_cpu_stats(frame: &mut Frame, area: Rect, cpu_stats: &CpuStats, cpu_history: &[VecDeque<f64>]) {
    let block = Block::default()
        .title(format!(" CPU ({} cores) ", cpu_stats.cores.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Handle empty cores case
    if cpu_stats.cores.is_empty() {
        let placeholder = Paragraph::new("Collecting CPU stats...")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Calculate how many cores we can fit - each row is 1 line tall
    let cores_per_row = 4;
    let rows_needed = (cpu_stats.cores.len() + cores_per_row - 1) / cores_per_row;

    // Each row is exactly 1 line
    let row_constraints: Vec<Constraint> = (0..rows_needed)
        .map(|_| Constraint::Length(1))
        .collect();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(inner);

    for (row_idx, row_area) in rows.iter().enumerate() {
        let col_constraints: Vec<Constraint> = (0..cores_per_row)
            .map(|_| Constraint::Percentage(100 / cores_per_row as u16))
            .collect();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_area);

        for (col_idx, col_area) in cols.iter().enumerate() {
            // Column-major order: cores go down columns first, then wrap to next column
            let core_idx = col_idx * rows_needed + row_idx;
            if core_idx < cpu_stats.cores.len() {
                let history = cpu_history.get(core_idx);
                render_cpu_core(frame, *col_area, &cpu_stats.cores[core_idx], history);
            }
        }
    }
}

fn render_cpu_core(frame: &mut Frame, area: Rect, core: &crate::collectors::CoreStats, history: Option<&VecDeque<f64>>) {
    // Determine if core is busy (blinker)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let blink = (now.as_millis() / 200) % 2 == 0;

    let indicator = if core.total_pct > 5.0 {
        if blink {
            "●"
        } else {
            "○"
        }
    } else {
        "○"
    };

    let color = if core.total_pct > 80.0 {
        Color::Red
    } else if core.total_pct > 50.0 {
        Color::Yellow
    } else if core.total_pct > 5.0 {
        Color::Green
    } else {
        Color::DarkGray
    };

    // Single line layout: [indicator C## pct%] [sparkline]
    // Label takes ~10 chars: "● C15 100%"
    let label_width = 10u16;
    let sparkline_width = area.width.saturating_sub(label_width + 1);

    if sparkline_width >= 5 && history.is_some() {
        // Split horizontally: label on left, sparkline on right
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(label_width),
                Constraint::Min(5),
            ])
            .split(area);

        // Render label
        let label = Line::from(vec![
            Span::styled(indicator, Style::default().fg(color)),
            Span::raw(format!(" C{:<2} {:>3.0}%", core.core_id, core.total_pct)),
        ]);
        let paragraph = Paragraph::new(label);
        frame.render_widget(paragraph, chunks[0]);

        // Render sparkline
        if let Some(hist) = history {
            if !hist.is_empty() {
                let width = chunks[1].width as usize;
                let start = if hist.len() > width {
                    hist.len() - width
                } else {
                    0
                };
                let data: Vec<u64> = hist.iter().skip(start).map(|&v| v as u64).collect();
                let sparkline = Sparkline::default()
                    .data(&data)
                    .style(Style::default().fg(Color::Cyan))
                    .bar_set(ratatui::symbols::bar::NINE_LEVELS);
                frame.render_widget(sparkline, chunks[1]);
            }
        }
    } else {
        // Not enough width for sparkline, just show label
        let label = Line::from(vec![
            Span::styled(indicator, Style::default().fg(color)),
            Span::raw(format!(" C{:<2} {:>3.0}%", core.core_id, core.total_pct)),
        ]);
        let paragraph = Paragraph::new(label);
        frame.render_widget(paragraph, area);
    }
}

fn render_memory_stats(frame: &mut Frame, area: Rect, mem_stats: &MemoryStats, _memory_history: &VecDeque<f64>) {
    let block = Block::default()
        .title(" Memory ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let total = mem_stats.total_bytes as f64;
    if total == 0.0 {
        return;
    }

    // Calculate memory segments (ARC is part of wired, so subtract it)
    let wired_non_arc = mem_stats.wired_bytes.saturating_sub(mem_stats.arc_total_bytes);
    let arc = mem_stats.arc_total_bytes;
    let active = mem_stats.active_bytes;
    let inactive = mem_stats.inactive_bytes;
    let laundry = mem_stats.laundry_bytes;
    let free = mem_stats.free_bytes;

    // Calculate percentages
    let wired_pct = (wired_non_arc as f64 / total * 100.0) as u16;
    let arc_pct = (arc as f64 / total * 100.0) as u16;
    let active_pct = (active as f64 / total * 100.0) as u16;
    let inactive_pct = (inactive as f64 / total * 100.0) as u16;
    let _laundry_pct = (laundry as f64 / total * 100.0) as u16;
    let _free_pct = (free as f64 / total * 100.0) as u16;

    // Format helper
    fn fmt_gb(bytes: u64) -> String {
        let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        if gb >= 10.0 {
            format!("{:.0}G", gb)
        } else {
            format!("{:.1}G", gb)
        }
    }

    // Row 1: Stacked bar visualization
    let bar_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };

    // Build the stacked bar as colored characters
    let bar_width = bar_area.width as usize;
    let mut bar_spans: Vec<Span> = Vec::new();

    // Calculate character widths for each segment
    let wired_chars = (wired_pct as usize * bar_width / 100).max(if wired_non_arc > 0 { 1 } else { 0 });
    let arc_chars = (arc_pct as usize * bar_width / 100).max(if arc > 0 { 1 } else { 0 });
    let active_chars = (active_pct as usize * bar_width / 100).max(if active > 0 { 1 } else { 0 });
    let inactive_chars = (inactive_pct as usize * bar_width / 100).max(if inactive > 0 { 1 } else { 0 });

    // Fill remaining with free
    let used_chars = wired_chars + arc_chars + active_chars + inactive_chars;
    let free_chars = bar_width.saturating_sub(used_chars);

    // Add segments with block characters
    if wired_chars > 0 {
        bar_spans.push(Span::styled("█".repeat(wired_chars), Style::default().fg(Color::Red)));
    }
    if arc_chars > 0 {
        bar_spans.push(Span::styled("█".repeat(arc_chars), Style::default().fg(Color::Blue)));
    }
    if active_chars > 0 {
        bar_spans.push(Span::styled("█".repeat(active_chars), Style::default().fg(Color::Green)));
    }
    if inactive_chars > 0 {
        bar_spans.push(Span::styled("█".repeat(inactive_chars), Style::default().fg(Color::Yellow)));
    }
    if free_chars > 0 {
        bar_spans.push(Span::styled("░".repeat(free_chars), Style::default().fg(Color::DarkGray)));
    }

    frame.render_widget(Paragraph::new(Line::from(bar_spans)), bar_area);

    // Row 2: Legend with values
    if inner.height > 1 {
        let legend_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        };

        let total_gb = mem_stats.total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        let legend = Line::from(vec![
            Span::styled("█", Style::default().fg(Color::Red)),
            Span::styled(format!("W:{} ", fmt_gb(wired_non_arc)), Style::default().fg(Color::DarkGray)),
            Span::styled("█", Style::default().fg(Color::Blue)),
            Span::styled(format!("ARC:{} ", fmt_gb(arc)), Style::default().fg(Color::DarkGray)),
            Span::styled("█", Style::default().fg(Color::Green)),
            Span::styled(format!("A:{} ", fmt_gb(active)), Style::default().fg(Color::DarkGray)),
            Span::styled("█", Style::default().fg(Color::Yellow)),
            Span::styled(format!("I:{} ", fmt_gb(inactive)), Style::default().fg(Color::DarkGray)),
            Span::styled("░", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("F:{} ", fmt_gb(free)), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("/{:.0}G", total_gb), Style::default().fg(Color::White)),
        ]);

        frame.render_widget(Paragraph::new(legend), legend_area);
    }

    // Row 3: Swap info if present
    if mem_stats.swap_total_bytes > 0 && inner.height > 2 {
        let swap_area = Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        };

        let swap_gb = mem_stats.swap_total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        let swap_used_gb = mem_stats.swap_used_bytes as f64 / 1024.0 / 1024.0 / 1024.0;

        let swap_color = if mem_stats.swap_used_pct > 50.0 {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let swap_text = format!("Swap: {:.1}/{:.1}G ({:.0}%)", swap_used_gb, swap_gb, mem_stats.swap_used_pct);
        frame.render_widget(Paragraph::new(swap_text).style(Style::default().fg(swap_color)), swap_area);
    }
}

fn render_network_stats(
    frame: &mut Frame,
    area: Rect,
    network_stats: &[NetworkStats],
    network_history: &std::collections::HashMap<String, VecDeque<f64>>,
) {
    let title = format!(" Network ({}) ", network_stats.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if network_stats.is_empty() {
        let placeholder = Paragraph::new("No network interfaces")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Format helper for bandwidth
    fn format_bw(bytes_per_sec: f64) -> String {
        if bytes_per_sec >= 1_000_000_000.0 {
            format!("{:>5.1}G", bytes_per_sec / 1_000_000_000.0)
        } else if bytes_per_sec >= 1_000_000.0 {
            format!("{:>5.1}M", bytes_per_sec / 1_000_000.0)
        } else if bytes_per_sec >= 1_000.0 {
            format!("{:>5.1}K", bytes_per_sec / 1_000.0)
        } else {
            format!("{:>5.0}B", bytes_per_sec)
        }
    }

    // Layout: interface list on left, combined chart on right
    // Text width: name(10) + rx_ind(1) + rx_bw(6) + space(1) + tx_ind(1) + tx_bw(6) = 25
    const TEXT_WIDTH: u16 = 25;

    let chart_width = if inner.width > TEXT_WIDTH + 2 {
        inner.width - TEXT_WIDTH
    } else {
        0
    };

    // Left side: interface list
    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: TEXT_WIDTH.min(inner.width),
        height: inner.height,
    };

    // Right side: combined chart (full height)
    let chart_area = Rect {
        x: inner.x + TEXT_WIDTH,
        y: inner.y,
        width: chart_width,
        height: inner.height,
    };

    // Render interface list
    let available_height = inner.height as usize;
    for (idx, iface) in network_stats.iter().take(available_height).enumerate() {
        let y_pos = list_area.y + idx as u16;
        let line_area = Rect {
            x: list_area.x,
            y: y_pos,
            width: list_area.width,
            height: 1,
        };

        // Indent members of aggregates
        let name_prefix = if iface.is_member { " └" } else { "" };
        let name_display = format!("{}{}", name_prefix, iface.name);

        // Determine if interface has traffic
        let has_rx = iface.rx_bytes_per_sec > 100.0;
        let has_tx = iface.tx_bytes_per_sec > 100.0;

        // Activity indicators with triangles
        let (rx_indicator, rx_color) = if has_rx {
            ("▼", Color::Green)
        } else {
            ("▽", Color::DarkGray)
        };

        let (tx_indicator, tx_color) = if has_tx {
            ("▲", Color::Yellow)
        } else {
            ("△", Color::DarkGray)
        };

        let rx_bw = format_bw(iface.rx_bytes_per_sec);
        let tx_bw = format_bw(iface.tx_bytes_per_sec);

        let name_color = if iface.is_aggregate {
            Color::White
        } else if iface.is_member {
            Color::Cyan
        } else {
            Color::White
        };

        let spans = vec![
            Span::styled(format!("{:<8}", name_display), Style::default().fg(name_color)),
            Span::styled(rx_indicator, Style::default().fg(rx_color)),
            Span::styled(format!("{}", rx_bw), Style::default().fg(if has_rx { Color::Green } else { Color::DarkGray })),
            Span::styled(tx_indicator, Style::default().fg(tx_color)),
            Span::styled(format!("{}", tx_bw), Style::default().fg(if has_tx { Color::Yellow } else { Color::DarkGray })),
        ];
        let text = Line::from(spans);
        frame.render_widget(Paragraph::new(text), line_area);
    }

    // Render combined chart on right side
    if chart_width > 3 && inner.height > 1 {
        // Calculate total bandwidth from non-member interfaces (avoid double-counting)
        let total_history: Vec<f64> = {
            let max_len = network_history.values()
                .map(|h| h.len())
                .max()
                .unwrap_or(0);

            if max_len == 0 {
                Vec::new()
            } else {
                // Sum histories from non-member interfaces only
                let non_member_ifaces: Vec<&str> = network_stats.iter()
                    .filter(|s| !s.is_member)
                    .map(|s| s.name.as_str())
                    .collect();

                (0..max_len).map(|i| {
                    non_member_ifaces.iter()
                        .filter_map(|name| {
                            network_history.get(*name).and_then(|h| {
                                if i < h.len() { Some(h[i]) } else { h.back().copied() }
                            })
                        })
                        .sum()
                }).collect()
            }
        };

        if !total_history.is_empty() {
            // Fixed window size based on chart width (2 data points per character with Braille)
            let window_size = (chart_width as usize) * 2;

            // Take only the most recent window_size points
            let start = if total_history.len() > window_size {
                total_history.len() - window_size
            } else {
                0
            };

            // Convert to (x, y) points - always use 0..window_size for X to keep fixed scale
            let data_points: Vec<(f64, f64)> = total_history.iter()
                .skip(start)
                .enumerate()
                .map(|(i, &v)| (i as f64, v))
                .collect();

            let max_val = data_points.iter().map(|(_, y)| *y).fold(1.0f64, f64::max);
            // Fixed X bounds - always use window_size so chart doesn't rescale
            let x_max = window_size as f64;

            // Format max value for Y axis label
            let max_label = if max_val >= 1_000_000_000.0 {
                format!("{:.1}G", max_val / 1_000_000_000.0)
            } else if max_val >= 1_000_000.0 {
                format!("{:.1}M", max_val / 1_000_000.0)
            } else if max_val >= 1_000.0 {
                format!("{:.1}K", max_val / 1_000.0)
            } else {
                format!("{:.0}B", max_val)
            };

            let datasets = vec![
                Dataset::default()
                    .marker(Marker::Braille)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&data_points),
            ];

            let chart = Chart::new(datasets)
                .x_axis(
                    Axis::default()
                        .bounds([0.0, x_max])
                        .style(Style::default().fg(Color::DarkGray))
                )
                .y_axis(
                    Axis::default()
                        .bounds([0.0, max_val])
                        .labels(vec![
                            Span::styled("0", Style::default().fg(Color::DarkGray)),
                            Span::styled(max_label, Style::default().fg(Color::DarkGray)),
                        ])
                        .style(Style::default().fg(Color::DarkGray))
                );

            frame.render_widget(chart, chart_area);
        }
    }
}

fn render_vm_list(frame: &mut Frame, area: Rect, vms: &[VmInfo]) {
    let title = format!(" bhyve VMs ({}) ", vms.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if vms.is_empty() {
        let paragraph = Paragraph::new("No VMs running")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(paragraph, inner);
        return;
    }

    // Format helper for memory
    fn format_mem(bytes: u64) -> String {
        let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        if gb >= 1.0 {
            format!("{:.1}G", gb)
        } else {
            let mb = bytes as f64 / 1024.0 / 1024.0;
            format!("{:.0}M", mb)
        }
    }

    let available_height = inner.height as usize;

    for (idx, vm) in vms.iter().take(available_height).enumerate() {
        let y_pos = inner.y + idx as u16;
        let line_area = Rect {
            x: inner.x,
            y: y_pos,
            width: inner.width,
            height: 1,
        };

        // Color based on CPU usage
        let cpu_color = if vm.cpu_pct > 80.0 {
            Color::Red
        } else if vm.cpu_pct > 50.0 {
            Color::Yellow
        } else if vm.cpu_pct > 5.0 {
            Color::Green
        } else {
            Color::DarkGray
        };

        // Format: ● name CPU% MEM
        let mem_str = format_mem(vm.memory_bytes);
        let spans = vec![
            Span::styled("● ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:<12}", vm.name), Style::default().fg(Color::White)),
            Span::styled(format!("{:>5.1}%", vm.cpu_pct), Style::default().fg(cpu_color)),
            Span::styled(format!(" {:>6}", mem_str), Style::default().fg(Color::Cyan)),
        ];

        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), line_area);
    }
}

fn render_jail_list(frame: &mut Frame, area: Rect, jails: &[JailInfo]) {
    let title = format!(" Jails ({}) ", jails.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if jails.is_empty() {
        let paragraph = Paragraph::new("No jails running")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(paragraph, area);
    } else {
        let items: Vec<ListItem> = jails
            .iter()
            .map(|jail| {
                let content = format!("● {} (JID: {})", jail.name, jail.jid);
                ListItem::new(content).style(Style::default().fg(Color::Green))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }
}
