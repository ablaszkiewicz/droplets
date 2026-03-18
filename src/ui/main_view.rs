use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::types::{time_ago, SPINNER};

pub fn draw(
    f: &mut Frame,
    state: &MainState,
    spin: usize,
    notification: &Option<(String, u32)>,
) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Layout: tab bar (1) + main content + footer (1) + notification (1 if present)
    let footer_height = if notification.is_some() { 2 } else { 1 };
    let chunks = Layout::vertical([
        Constraint::Length(1),         // tab bar
        Constraint::Min(3),            // content
        Constraint::Length(footer_height), // footer
    ])
    .split(area);

    draw_tab_bar(f, chunks[0], state.tab);

    match state.tab {
        Tab::Droplets => draw_droplets_tab(f, chunks[1], &state.droplets, spin),
        Tab::Config => draw_config_tab(f, chunks[1], &state.config, spin),
    }

    draw_footer(f, chunks[2], state, notification);
}

fn draw_tab_bar(f: &mut Frame, area: Rect, active: Tab) {
    let droplets_style = if active == Tab::Droplets {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let config_style = if active == Tab::Config {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(" Droplets ", droplets_style),
        Span::raw("  "),
        Span::styled(" Config ", config_style),
        Span::styled(
            "                                        Tab ↹",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ── Droplets Tab ────────────────────────────────────────────────────────────

fn draw_droplets_tab(f: &mut Frame, area: Rect, ds: &DropletsState, spin: usize) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);

    draw_droplets_list(f, chunks[0], ds, spin);
    draw_droplet_detail(f, chunks[1], ds, spin);
}

fn draw_droplets_list(f: &mut Frame, area: Rect, ds: &DropletsState, spin: usize) {
    let focused = ds.focus == DFocus::List;
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Droplets ")
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Droplets
    for (i, d) in ds.items.iter().enumerate() {
        let is_selected = i == ds.selected && focused;
        let is_deleting = ds.deleting.contains(&d.id);

        let (icon, icon_color) = if is_deleting {
            (SPINNER[spin], Color::Yellow)
        } else if d.status == "active" {
            ("●", Color::Green)
        } else {
            (SPINNER[spin], Color::Yellow)
        };

        let status_text = if is_deleting {
            "(destroying)".to_string()
        } else {
            format!("({})", d.status)
        };

        let name_style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
            Span::styled(&d.name, name_style),
            Span::raw(" "),
            Span::styled(status_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Creating
    let creating_start = ds.items.len();
    for (i, name) in ds.creating.iter().enumerate() {
        let idx = creating_start + i;
        let is_selected = idx == ds.selected && focused;

        let name_style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[spin]),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(name.as_str(), name_style),
            Span::styled(" (creating)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Separator
    if !ds.items.is_empty() || !ds.creating.is_empty() {
        lines.push(Line::raw(""));
    }

    // Create option
    let create_idx = ds.items.len() + ds.creating.len();
    let is_create_selected = create_idx == ds.selected && focused;
    let create_style = if is_create_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Green)
    };
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled("+ Create new droplet", create_style),
    ]));

    // Loading indicator
    if ds.loading && ds.items.is_empty() && ds.creating.is_empty() {
        lines.insert(
            0,
            Line::from(vec![
                Span::styled(
                    format!(" {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Loading..."),
            ]),
        );
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_droplet_detail(f: &mut Frame, area: Rect, ds: &DropletsState, spin: usize) {
    let focused = ds.focus == DFocus::Detail;
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let selected_droplet = if ds.selected < ds.items.len() {
        Some(&ds.items[ds.selected])
    } else {
        None
    };

    let title = match selected_droplet {
        Some(d) => format!(" {} ", d.name),
        None => " Selected Droplet ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(d) = selected_droplet else {
        let msg = if ds.loading {
            format!("{} Loading...", SPINNER[spin])
        } else {
            "None".to_string()
        };
        let center_y = inner.y + inner.height / 2;
        let center_x = inner.x + (inner.width.saturating_sub(msg.len() as u16)) / 2;
        let r = Rect::new(center_x, center_y, msg.len() as u16, 1);
        f.render_widget(
            Paragraph::new(Line::styled(msg, Style::default().fg(Color::DarkGray))),
            r,
        );
        return;
    };

    let is_deleting = ds.deleting.contains(&d.id);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(ref ip) = d.ip {
        lines.push(Line::from(vec![
            Span::styled("  IP:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(ip.as_str(), Style::default().fg(Color::Cyan)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  Status:  ", Style::default().fg(Color::DarkGray)),
        if is_deleting {
            Span::styled(
                format!("{} destroying", SPINNER[spin]),
                Style::default().fg(Color::Yellow),
            )
        } else if d.status == "active" {
            Span::styled("active", Style::default().fg(Color::Green))
        } else {
            Span::styled(&d.status, Style::default().fg(Color::Yellow))
        },
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Region:  ", Style::default().fg(Color::DarkGray)),
        Span::raw(&d.region),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Size:    ", Style::default().fg(Color::DarkGray)),
        Span::raw(&d.size),
    ]));

    let ago = time_ago(&d.created_at);
    if !ago.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Created: ", Style::default().fg(Color::DarkGray)),
            Span::raw(ago),
        ]));
    }

    // Actions (only when focused and not deleting)
    if !is_deleting {
        lines.push(Line::raw(""));

        let mut action_idx = 0;

        if d.ip.is_some() {
            let style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Copy SSH command", style),
            ]));
            action_idx += 1;
        }

        let del_style = if focused && ds.detail_selected == action_idx {
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Red)
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Delete droplet", del_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Config Tab ──────────────────────────────────────────────────────────────

fn draw_config_tab(f: &mut Frame, area: Rect, cfg: &ConfigViewState, spin: usize) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    draw_config_panel(
        f,
        chunks[0],
        "GitHub SSH",
        &cfg.github,
        cfg.focus == CFocus::Github,
        spin,
    );
    draw_config_panel(
        f,
        chunks[1],
        "DigitalOcean API",
        &cfg.digitalocean,
        cfg.focus == CFocus::DigitalOcean,
        spin,
    );
}

fn draw_config_panel(
    f: &mut Frame,
    area: Rect,
    title: &str,
    info: &KeyCheckInfo,
    focused: bool,
    spin: usize,
) {
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    // Status
    let (icon, icon_color, status_text) = match info.status {
        KeyStatus::Unknown => ("?", Color::DarkGray, "Unknown"),
        KeyStatus::Checking => (SPINNER[spin], Color::Yellow, "Checking..."),
        KeyStatus::Ok => ("✓", Color::Green, "Ok"),
        KeyStatus::Error => ("✗", Color::Red, "Error"),
    };
    lines.push(Line::from(vec![
        Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(
            status_text,
            Style::default().fg(icon_color),
        ),
    ]));

    if let Some(ref msg) = info.message {
        lines.push(Line::styled(
            format!("    {msg}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.push(Line::raw(""));

    // Actions
    let actions = ["Set up again", "Test now"];
    for (i, action) in actions.iter().enumerate() {
        let style = if focused && info.selected == i {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if focused && info.selected == i {
            "  > "
        } else {
            "    "
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(*action, style),
        ]));
    }

    lines.push(Line::raw(""));

    // Next check countdown
    let secs = (info.next_check as f64 * 0.1).ceil() as u32;
    lines.push(Line::styled(
        format!("  Next check in {secs}s"),
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Footer ──────────────────────────────────────────────────────────────────

fn draw_footer(
    f: &mut Frame,
    area: Rect,
    state: &MainState,
    notification: &Option<(String, u32)>,
) {
    let mut lines: Vec<Line> = Vec::new();

    // Keybindings
    let bindings: Vec<(&str, &str)> = match state.tab {
        Tab::Droplets => match state.droplets.focus {
            DFocus::List => vec![
                ("↑↓", "navigate"),
                ("Enter/→", "select"),
                ("Tab", "config"),
                ("q", "quit"),
            ],
            DFocus::Detail => vec![
                ("↑↓", "navigate"),
                ("Enter", "action"),
                ("←/Esc", "back"),
                ("Tab", "config"),
                ("q", "quit"),
            ],
        },
        Tab::Config => vec![
            ("←→", "switch"),
            ("↑↓", "navigate"),
            ("Enter", "action"),
            ("Tab", "droplets"),
            ("q", "quit"),
        ],
    };

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, (key, desc)) in bindings.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!(" {desc}")));
    }
    lines.push(Line::from(spans));

    // Notification
    if let Some((msg, _)) = notification {
        lines.push(Line::styled(
            format!(" {msg}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
