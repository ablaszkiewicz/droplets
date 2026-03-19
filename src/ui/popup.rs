use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::types::{MACHINES, REGIONS, SPINNER, SnapshotInfo};

/// Renders the create droplet flow as a full-screen view (not an overlay).
pub fn draw_create_fullscreen(f: &mut Frame, state: &CreatePopupState, _spin: usize) {
    let area = f.area();
    f.render_widget(Clear, area);

    let chunks = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(match &state.step {
            CreateStep::Main { .. } => " Create Droplet ",
            CreateStep::Region { .. } => " Select Region ",
            CreateStep::Machine { .. } => " Select Machine ",
            CreateStep::Snapshot { .. } => " Select Image ",
            CreateStep::Name(_) => " Droplet Name ",
        })
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    match &state.step {
        CreateStep::Main { selected } => {
            draw_create_main_content(f, inner, state, *selected);
        }
        CreateStep::Region { selected } => {
            draw_create_region_content(f, inner, *selected);
        }
        CreateStep::Machine { selected } => {
            draw_create_machine_content(f, inner, *selected);
        }
        CreateStep::Snapshot { selected } => {
            draw_create_snapshot_content(f, inner, &state.snapshots, *selected);
        }
        CreateStep::Name(input) => {
            draw_create_name_content(f, inner, input);
        }
    }

    // Footer
    let footer_text = match &state.step {
        CreateStep::Main { .. } => " ↑↓ navigate │ Enter select │ Esc cancel",
        CreateStep::Region { .. } | CreateStep::Machine { .. } | CreateStep::Snapshot { .. } => " ↑↓ navigate │ Enter select │ Esc back",
        CreateStep::Name(_) => " Enter save │ Esc cancel",
    };
    f.render_widget(
        Paragraph::new(Line::styled(footer_text, Style::default().fg(Color::DarkGray))),
        chunks[1],
    );
}

fn draw_create_main_content(f: &mut Frame, area: Rect, state: &CreatePopupState, selected: usize) {
    let region_name = REGIONS[state.region_idx].name;
    let region_slug = REGIONS[state.region_idx].slug;
    let machine_name = MACHINES[state.machine_idx].name;
    let image_name = match state.snapshot_idx {
        None => "Ubuntu 24.04 (base)".to_string(),
        Some(i) => {
            let s = &state.snapshots[i];
            format!("{} ({:.1} GB)", s.name, s.size_gigabytes)
        }
    };

    let items = [
        format!("Region:  {region_name} ({region_slug})"),
        format!("Machine: {machine_name}"),
        format!("Image:   {image_name}"),
        format!("Name:    {}", state.name),
        String::new(),
        "Create".to_string(),
        "Cancel".to_string(),
    ];

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, item) in items.iter().enumerate() {
        if i == 4 {
            lines.push(Line::raw(""));
            continue;
        }
        let real_idx = if i > 4 { i - 1 } else { i };
        let is_sel = real_idx == selected;

        let style = if is_sel {
            if i == 5 {
                Style::default().fg(Color::Green).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
            }
        } else if i == 5 {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(item.as_str(), style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn draw_create_snapshot_content(f: &mut Frame, area: Rect, snapshots: &[SnapshotInfo], selected: usize) {
    let mut lines: Vec<Line> = vec![Line::raw("")];

    // First option: no snapshot (base image)
    let is_sel = selected == 0;
    let style = if is_sel {
        Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::White)
    };
    let prefix = if is_sel { "  > " } else { "    " };
    lines.push(Line::from(vec![
        Span::raw(prefix),
        Span::styled("None (Ubuntu 24.04 base image)", style),
    ]));

    for (i, snap) in snapshots.iter().enumerate() {
        let idx = i + 1;
        let is_sel = idx == selected;
        let style = if is_sel {
            Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{}  ({:.1} GB)", snap.name, snap.size_gigabytes), style),
        ]));
    }

    if snapshots.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "    No snapshots available",
            Style::default().fg(Color::DarkGray),
        ));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn draw_create_region_content(f: &mut Frame, area: Rect, selected: usize) {
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, r) in REGIONS.iter().enumerate() {
        let is_sel = i == selected;
        let style = if is_sel {
            Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{} ({})", r.name, r.slug), style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_create_machine_content(f: &mut Frame, area: Rect, selected: usize) {
    let available: Vec<&crate::types::MachineSize> = MACHINES.iter().filter(|m| m.available).collect();
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, m) in available.iter().enumerate() {
        let is_sel = i == selected;
        let style = if is_sel {
            Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{}  {}  ${:.3}/hr", m.name, m.desc, m.hourly_price), style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_create_name_content(f: &mut Frame, area: Rect, input: &TextInput) {
    let display: String = input.display();
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  > "),
            Span::styled(display, Style::default().fg(Color::White)),
        ]),
    ];

    let cursor_x = area.x + 4 + input.cursor as u16;
    let cursor_y = area.y + 1;
    if cursor_x < area.x + area.width {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    f.render_widget(Paragraph::new(lines), area);
}

pub fn draw(f: &mut Frame, popup: &Popup, spin: usize) {
    match popup {
        Popup::Confirm { message, .. } => draw_confirm(f, message),
        Popup::Message(msg) => draw_message(f, msg),
        Popup::CreateDroplet(state) => draw_create(f, state, spin),
        Popup::GithubSetup(phase) => draw_github_setup(f, phase, spin),
        Popup::DoSetup(phase) => draw_do_setup(f, phase, spin),
Popup::SnapshotName { input, .. } => draw_snapshot_name(f, input),
        Popup::RenameSnapshot { input, .. } => draw_rename_snapshot(f, input),
        Popup::RenameDroplet { input, .. } => draw_rename_droplet(f, input),
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

// ── Confirm ─────────────────────────────────────────────────────────────────

fn draw_confirm(f: &mut Frame, message: &str) {
    let rect = centered(f.area(), 40, 6);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let lines = vec![
        Line::raw(""),
        Line::from(vec![Span::raw(format!("  {message}"))]),
        Line::raw(""),
        Line::styled("  y/n", Style::default().fg(Color::DarkGray)),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

// ── Message ─────────────────────────────────────────────────────────────────

fn draw_message(f: &mut Frame, message: &str) {
    let line_count = message.lines().count() as u16;
    let max_line = message.lines().map(|l| l.len()).max().unwrap_or(20) as u16;
    let w = (max_line + 8).min(60);
    let h = (line_count + 5).min(20);
    let rect = centered(f.area(), w, h);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Info ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for line in message.lines() {
        lines.push(Line::raw(format!("  {line}")));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  Enter close",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Create Droplet ──────────────────────────────────────────────────────────

fn draw_create(f: &mut Frame, state: &CreatePopupState, _spin: usize) {
    match &state.step {
        CreateStep::Main { selected } => draw_create_main(f, state, *selected),
        CreateStep::Region { selected } => draw_create_region(f, *selected),
        CreateStep::Machine { selected } => draw_create_machine(f, *selected),
        CreateStep::Snapshot { selected } => draw_create_snapshot(f, &state.snapshots, *selected),
        CreateStep::Name(input) => draw_create_name(f, input),
    }
}

fn draw_create_main(f: &mut Frame, state: &CreatePopupState, selected: usize) {
    let rect = centered(f.area(), 55, 16);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Create Droplet ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let region_name = REGIONS[state.region_idx].name;
    let region_slug = REGIONS[state.region_idx].slug;
    let machine_name = MACHINES[state.machine_idx].name;
    let image_name = match state.snapshot_idx {
        None => "Ubuntu 24.04 (base)".to_string(),
        Some(i) => {
            let s = &state.snapshots[i];
            format!("{} ({:.1} GB)", s.name, s.size_gigabytes)
        }
    };

    let items = [
        format!("Region:  {region_name} ({region_slug})"),
        format!("Machine: {machine_name}"),
        format!("Image:   {image_name}"),
        format!("Name:    {}", state.name),
        String::new(), // separator
        "Create".to_string(),
        "Cancel".to_string(),
    ];

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, item) in items.iter().enumerate() {
        if i == 4 {
            lines.push(Line::raw(""));
            continue;
        }
        let real_idx = if i > 4 { i - 1 } else { i }; // skip separator in index
        let is_sel = real_idx == selected;

        let style = if is_sel {
            if i == 6 {
                // Cancel
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            } else if i == 5 {
                // Create
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            }
        } else if i == 5 {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(item.as_str(), style),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  ↑↓ navigate  Enter select  Esc back",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_create_snapshot(f: &mut Frame, snapshots: &[SnapshotInfo], selected: usize) {
    let h = (snapshots.len() + 1) as u16 + 5; // +1 for "None" option
    let rect = centered(f.area(), 55, h.max(8));
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Image ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];

    // "None" option
    let is_sel = selected == 0;
    let style = if is_sel {
        Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::White)
    };
    let prefix = if is_sel { "  > " } else { "    " };
    lines.push(Line::from(vec![
        Span::raw(prefix),
        Span::styled("None (Ubuntu 24.04 base image)", style),
    ]));

    for (i, snap) in snapshots.iter().enumerate() {
        let idx = i + 1;
        let is_sel = idx == selected;
        let style = if is_sel {
            Style::default().fg(Color::White).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{}  ({:.1} GB)", snap.name, snap.size_gigabytes), style),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  ↑↓ navigate  Enter select  Esc back",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_create_region(f: &mut Frame, selected: usize) {
    let h = REGIONS.len() as u16 + 5;
    let rect = centered(f.area(), 40, h);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Region ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, r) in REGIONS.iter().enumerate() {
        let is_sel = i == selected;
        let style = if is_sel {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{} ({})", r.name, r.slug), style),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  ↑↓ navigate  Enter select  Esc back",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_create_machine(f: &mut Frame, selected: usize) {
    let available: Vec<&crate::types::MachineSize> =
        MACHINES.iter().filter(|m| m.available).collect();
    let h = available.len() as u16 + 5;
    let rect = centered(f.area(), 50, h);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Machine ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, m) in available.iter().enumerate() {
        let is_sel = i == selected;
        let style = if is_sel {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "  > " } else { "    " };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{}  {}  ${:.3}/hr", m.name, m.desc, m.hourly_price), style),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  ↑↓ navigate  Enter select  Esc back",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_create_name(f: &mut Frame, input: &TextInput) {
    let rect = centered(f.area(), 45, 7);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Droplet Name ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let display = input.display();
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  > "),
            Span::styled(&display, Style::default().fg(Color::White)),
        ]),
        Line::raw(""),
        Line::styled(
            "  Enter save  Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let cursor_x = inner.x + 4 + input.cursor as u16;
    let cursor_y = inner.y + 1;
    if cursor_x < inner.x + inner.width {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── GitHub Setup ────────────────────────────────────────────────────────────

fn draw_github_setup(f: &mut Frame, phase: &GithubSetupPhase, spin: usize) {
    let rect = centered(f.area(), 55, 14);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" GitHub SSH Setup ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];

    match phase {
        GithubSetupPhase::Generating => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Generating SSH key..."),
            ]));
        }
        GithubSetupPhase::Ready { copied, .. } => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw("SSH key generated!"),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::raw("  Add this key to GitHub:"));
            lines.push(Line::styled(
                "  https://github.com/settings/ssh/new",
                Style::default().fg(Color::Cyan),
            ));
            lines.push(Line::raw(""));
            if *copied {
                lines.push(Line::from(vec![
                    Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                    Span::raw("Copied to clipboard"),
                ]));
            } else {
                lines.push(Line::raw("  [C] Copy public key"));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter test  Esc close",
                Style::default().fg(Color::DarkGray),
            ));
        }
        GithubSetupPhase::Testing { .. } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Testing GitHub SSH connection..."),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Esc close",
                Style::default().fg(Color::DarkGray),
            ));
        }
        GithubSetupPhase::Failed { error, copied, .. } => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("Test failed"),
            ]));
            lines.push(Line::styled(
                format!("    {error}"),
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::raw(""));
            if *copied {
                lines.push(Line::from(vec![
                    Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                    Span::raw("Copied to clipboard"),
                ]));
            } else {
                lines.push(Line::raw("  [C] Copy public key"));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter retest  Esc close",
                Style::default().fg(Color::DarkGray),
            ));
        }
        GithubSetupPhase::Done(msg) => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(msg.as_str()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter close",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── DO Setup ────────────────────────────────────────────────────────────────

fn draw_do_setup(f: &mut Frame, phase: &DoSetupPhase, spin: usize) {
    let rect = centered(f.area(), 65, 16);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" DigitalOcean API Setup ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];

    match phase {
        DoSetupPhase::Input(input) => {
            lines.push(Line::raw("  Enter your DigitalOcean API token:"));
            lines.push(Line::styled(
                "  https://cloud.digitalocean.com/account/api/tokens",
                Style::default().fg(Color::Cyan),
            ));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Required scopes:",
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::styled(
                "  account:read  droplet:create/read/delete",
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::styled(
                "  image:create/read  snapshot:read/delete  ssh_key:create/read",
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::raw(""));

            let display: String = input.display();
            lines.push(Line::from(vec![
                Span::raw("  > "),
                Span::styled(display, Style::default().fg(Color::White)),
            ]));

            let cursor_x = inner.x + 4 + input.cursor as u16;
            let cursor_y = inner.y + 8;
            if cursor_x < inner.x + inner.width {
                f.set_cursor_position((cursor_x, cursor_y));
            }

            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter save  Esc cancel",
                Style::default().fg(Color::DarkGray),
            ));
        }
        DoSetupPhase::Testing => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Testing API key..."),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Esc close",
                Style::default().fg(Color::DarkGray),
            ));
        }
        DoSetupPhase::Failed(error) => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("Test failed"),
            ]));
            lines.push(Line::styled(
                format!("    {error}"),
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter try again  Esc close",
                Style::default().fg(Color::DarkGray),
            ));
        }
        DoSetupPhase::Done(msg) => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(msg.as_str()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "  Enter close",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Snapshot Name ────────────────────────────────────────────────────────

fn draw_snapshot_name(f: &mut Frame, input: &TextInput) {
    let rect = centered(f.area(), 50, 7);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Snapshot Name ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let display = input.display();
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  > "),
            Span::styled(&display, Style::default().fg(Color::White)),
        ]),
        Line::raw(""),
        Line::styled(
            "  Enter save  Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let cursor_x = inner.x + 4 + input.cursor as u16;
    let cursor_y = inner.y + 1;
    if cursor_x < inner.x + inner.width {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Rename Snapshot ──────────────────────────────────────────────────────

fn draw_rename_snapshot(f: &mut Frame, input: &TextInput) {
    let rect = centered(f.area(), 50, 7);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename Snapshot ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let display = input.display();
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  > "),
            Span::styled(&display, Style::default().fg(Color::White)),
        ]),
        Line::raw(""),
        Line::styled(
            "  Enter save  Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let cursor_x = inner.x + 4 + input.cursor as u16;
    let cursor_y = inner.y + 1;
    if cursor_x < inner.x + inner.width {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Rename Droplet ──────────────────────────────────────────────────────

fn draw_rename_droplet(f: &mut Frame, input: &TextInput) {
    let rect = centered(f.area(), 50, 7);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename Droplet ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let display = input.display();
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  > "),
            Span::styled(&display, Style::default().fg(Color::White)),
        ]),
        Line::raw(""),
        Line::styled(
            "  Enter save  Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let cursor_x = inner.x + 4 + input.cursor as u16;
    let cursor_y = inner.y + 1;
    if cursor_x < inner.x + inner.width {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Port Input ──────────────────────────────────────────────────────────────

