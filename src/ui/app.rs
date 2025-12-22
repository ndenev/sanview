use crate::collectors::{CpuStats, MemoryStats};
use crate::ui::components::{render_front_panel, render_system_overview};
use crate::ui::state::AppState;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn run_tui(state: Arc<Mutex<AppState>>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the UI loop
    let result = run_app(&mut terminal, state);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, state: Arc<Mutex<AppState>>) -> Result<()> {
    loop {
        // Update terminal width in state for dynamic history sizing
        let terminal_size = terminal.size()?;
        {
            let mut state_guard = state.lock().unwrap();
            state_guard.set_terminal_width(terminal_size.width);
        }

        // Clone state for rendering
        let current_state = {
            let state_guard = state.lock().unwrap();
            state_guard.clone()
        };

        // Render
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),      // Header
                    Constraint::Percentage(30), // System stats (top)
                    Constraint::Min(12),        // Drive array (bottom)
                    Constraint::Length(3),      // Footer
                ])
                .split(frame.size());

            // Header
            render_header(frame, chunks[0], &current_state);

            // System stats section (CPU, Memory, VMs, Jails)
            let empty_cpu = CpuStats { cores: Vec::new() };
            let empty_mem = MemoryStats {
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
            };

            render_system_overview(
                frame,
                chunks[1],
                current_state.cpu_stats.as_ref().unwrap_or(&empty_cpu),
                current_state.memory_stats.as_ref().unwrap_or(&empty_mem),
                &current_state.network_stats,
                &current_state.vms,
                &current_state.jails,
                &current_state.cpu_history,
                &current_state.memory_history,
                &current_state.arc_size_history,
                &current_state.arc_ratio_history,
                &current_state.network_history,
            );

            // Drive array at bottom with history sparklines
            render_front_panel(
                frame,
                chunks[2],
                &current_state.multipath_devices,
                &current_state.storage_iops_history,
                &current_state.storage_read_bw_history,
                &current_state.storage_write_bw_history,
                &current_state.storage_busy_history,
                &current_state.drive_busy_history,
            );

            // Footer
            render_footer(frame, chunks[3], &current_state);
        })?;

        // Handle input with timeout to allow for periodic updates
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if handle_key_event(key, &state) {
                    break;  // User requested quit
                }
            }
        }

        // Check if app should quit
        {
            let state_guard = state.lock().unwrap();
            if state_guard.should_quit {
                break;
            }
        }
    }

    Ok(())
}

fn render_header(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let elapsed = state.last_update.elapsed();
    let header_text = Line::from(vec![
        Span::styled(
            "SANVIEW",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" - FreeBSD Storage Array Monitor  "),
        Span::styled(
            format!("Updated: {:.1}s ago", elapsed.as_secs_f64()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    frame.render_widget(header, area);
}

fn render_footer(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let footer_text = Line::from(vec![
        Span::raw("[Q]uit / [Esc]  "),
        Span::styled(
            format!(
                "{} multipath devices, {} standalone",
                state.multipath_devices.len(),
                state.standalone_disks.len()
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let footer = Paragraph::new(footer_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    frame.render_widget(footer, area);
}

fn handle_key_event(key: KeyEvent, state: &Arc<Mutex<AppState>>) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            let mut state_guard = state.lock().unwrap();
            state_guard.quit();
            true
        }
        _ => false,
    }
}
