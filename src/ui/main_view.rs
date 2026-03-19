use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::types::{time_ago, seconds_since, hourly_price_for_size, snapshot_monthly_cost, LocalStatus, StepStatus, SPINNER};

pub fn draw(
    f: &mut Frame,
    state: &MainState,
    spin: usize,
    notification: &Option<(String, u32)>,
) {
    let area = f.area();
    f.render_widget(Clear, area);

    let footer_height = if notification.is_some() { 2 } else { 1 };
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(footer_height),
    ])
    .split(area);

    draw_tab_bar(f, chunks[0], state.tab);

    match state.tab {
        Tab::Droplets => draw_droplets_tab(f, chunks[1], &state.droplets, spin),
        Tab::Snapshots => draw_snapshots_tab(f, chunks[1], &state.snapshots, spin),
        Tab::Config => draw_config_tab(f, chunks[1], &state.config, spin),
    }

    draw_footer(f, chunks[2], state, notification);
}

fn draw_tab_bar(f: &mut Frame, area: Rect, active: Tab) {
    let tab_style = |tab: Tab| {
        if active == tab {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(" Droplets ", tab_style(Tab::Droplets)),
        Span::raw("  "),
        Span::styled(" Snapshots ", tab_style(Tab::Snapshots)),
        Span::raw("  "),
        Span::styled(" Config ", tab_style(Tab::Config)),
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
        Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]).split(area);

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
    let views = ds.registry.views();

    for (i, view) in views.iter().enumerate() {
        let is_selected = i == ds.selected;

        let (icon, icon_color) = match view.local_status {
            LocalStatus::Creating => (SPINNER[spin], Color::Yellow),
            LocalStatus::Deleting => (SPINNER[spin], Color::Yellow),
            LocalStatus::Normal => {
                if let Some(api) = &view.api {
                    if api.status == "active" {
                        if view.provision.is_done() {
                            ("●", Color::Green)
                        } else if view.provision.error.is_some() {
                            ("●", Color::Red)
                        } else {
                            (SPINNER[spin], Color::Cyan)
                        }
                    } else {
                        (SPINNER[spin], Color::Yellow)
                    }
                } else {
                    ("?", Color::DarkGray)
                }
            }
        };

        let status_text = match view.local_status {
            LocalStatus::Creating => "(creating)".to_string(),
            LocalStatus::Deleting => "(destroying)".to_string(),
            LocalStatus::Normal => {
                if let Some(api) = &view.api {
                    if api.status == "active" {
                        let label = view.provision.overall_label();
                        format!("({})", label)
                    } else {
                        format!("({})", api.status)
                    }
                } else {
                    String::new()
                }
            }
        };

        let name_style = if is_selected && focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
            Span::styled(view.name.clone(), name_style),
            Span::raw(" "),
            Span::styled(status_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Separator
    if !views.is_empty() {
        lines.push(Line::raw(""));
    }

    // Create option
    let create_idx = views.len();
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
    if ds.loading && views.is_empty() {
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

// ── Detail: 3 bordered sub-windows ──────────────────────────────────────────

fn draw_droplet_detail(f: &mut Frame, area: Rect, ds: &DropletsState, spin: usize) {
    let selected_view = ds.registry.get_by_index(ds.selected);

    if selected_view.is_none() {
        let border_style = Style::default().fg(Color::DarkGray);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Selected Droplet ")
            .border_style(border_style);
        let inner = block.inner(area);
        f.render_widget(block, area);

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
    }

    let view = selected_view.unwrap();

    // Split into 3 columns
    let cols = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Percentage(40),
    ])
    .split(area);

    draw_detail_info_window(f, cols[0], view, ds, spin);
    draw_detail_provision_window(f, cols[1], view, ds, spin);
    draw_detail_log_window(f, cols[2], view, ds, spin);
}

fn draw_detail_info_window(
    f: &mut Frame,
    area: Rect,
    view: &crate::types::DropletView,
    ds: &DropletsState,
    spin: usize,
) {
    let focused = ds.focus == DFocus::DetailInfo;
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Machine ")
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    if let Some(api) = &view.api {
        if let Some(ref ip) = api.ip {
            lines.push(Line::from(vec![
                Span::styled(" IP:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(ip.as_str(), Style::default().fg(Color::Cyan)),
            ]));
        }

        let do_status_span = match view.local_status {
            LocalStatus::Deleting => Span::styled(
                format!("{} destroying", SPINNER[spin]),
                Style::default().fg(Color::Yellow),
            ),
            _ => {
                if api.status == "active" {
                    Span::styled("active", Style::default().fg(Color::Green))
                } else {
                    Span::styled(api.status.clone(), Style::default().fg(Color::Yellow))
                }
            }
        };

        lines.push(Line::from(vec![
            Span::styled(" DO:  ", Style::default().fg(Color::DarkGray)),
            do_status_span,
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Rgn: ", Style::default().fg(Color::DarkGray)),
            Span::raw(api.region.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Size:", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(" {}", api.size)),
        ]));

        let ago = time_ago(&api.created_at);
        if !ago.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(" Age: ", Style::default().fg(Color::DarkGray)),
                Span::raw(ago),
            ]));
        }

        if let Some(rate) = hourly_price_for_size(&api.size) {
            lines.push(Line::from(vec![
                Span::styled(" Rate:", Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" ${:.3}/hr", rate), Style::default().fg(Color::Yellow)),
            ]));
            if let Some(secs) = seconds_since(&api.created_at) {
                let hours = secs as f64 / 3600.0;
                let cost = hours * rate;
                lines.push(Line::from(vec![
                    Span::styled(" Cost:", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" ~${:.2}", cost), Style::default().fg(Color::Yellow)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            format!(" {} Waiting for API...", SPINNER[spin]),
            Style::default().fg(Color::Yellow),
        )]));
    }

    // ── Actions ──
    let is_deleting = view.local_status == LocalStatus::Deleting;
    if !is_deleting && view.api.is_some() {
        lines.push(Line::raw(""));

        let mut action_idx = 0;

        if view.api.as_ref().and_then(|a| a.ip.as_ref()).is_some() {
            let style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("Copy SSH cmd", style),
            ]));
            action_idx += 1;

            let open_style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("Open SSH in terminal", open_style),
            ]));
            action_idx += 1;

            // Map .droplet hosts entry
            let hosts_mapped = view.hosts_mapped;
            let hosts_label = if hosts_mapped {
                format!("Unmap {}.droplet", view.name)
            } else {
                format!("Map {}.droplet", view.name)
            };
            let hosts_color = if hosts_mapped { Color::Green } else { Color::White };
            let hosts_style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(hosts_color)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(hosts_color)
            };
            let hosts_icon = if hosts_mapped { "● " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(hosts_icon, Style::default().fg(Color::Green)),
                Span::styled(hosts_label, hosts_style),
            ]));
            action_idx += 1;

            // Snapshot this droplet
            let snap_style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("Snapshot this droplet", snap_style),
            ]));
            action_idx += 1;

            // Rename droplet
            let rename_style = if focused && ds.detail_selected == action_idx {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("Rename droplet", rename_style),
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
            Span::raw(" "),
            Span::styled("Delete droplet", del_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_detail_provision_window(
    f: &mut Frame,
    area: Rect,
    view: &crate::types::DropletView,
    ds: &DropletsState,
    spin: usize,
) {
    let focused = ds.focus == DFocus::DetailProvision;
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Provisioning ")
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    for (i, step) in view.provision.steps.iter().enumerate() {
        let (icon, color) = match &step.status {
            StepStatus::Pending => ("○", Color::DarkGray),
            StepStatus::Running => (SPINNER[spin], Color::Cyan),
            StepStatus::Done => ("✓", Color::Green),
            StepStatus::Failed(_) => ("✗", Color::Red),
        };

        let is_step_selected = focused && i == ds.provision_selected;

        let name_color = if is_step_selected {
            Color::White
        } else {
            match &step.status {
                StepStatus::Pending => Color::DarkGray,
                StepStatus::Running => Color::White,
                StepStatus::Done => Color::Green,
                StepStatus::Failed(_) => Color::Red,
            }
        };

        let name_style = if is_step_selected {
            Style::default()
                .fg(name_color)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(name_color)
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(color)),
            Span::styled(step.name, name_style),
        ]));

        if let StepStatus::Failed(err) = &step.status {
            let max_w = inner.width.saturating_sub(6) as usize;
            let display = if err.len() > max_w {
                format!("     {}…", &err[..max_w.saturating_sub(1)])
            } else {
                format!("     {err}")
            };
            lines.push(Line::styled(display, Style::default().fg(Color::Red)));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_detail_log_window(
    f: &mut Frame,
    area: Rect,
    view: &crate::types::DropletView,
    ds: &DropletsState,
    spin: usize,
) {
    // Determine which step's logs to show
    let step_idx = if ds.focus == DFocus::DetailProvision {
        ds.provision_selected
    } else {
        view.provision.most_recent_step()
    };

    let step_name = view
        .provision
        .steps
        .get(step_idx)
        .map(|s| s.name)
        .unwrap_or("Log");

    let focused = false; // Log window is not directly navigable
    let border_style = if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {step_name} "))
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Show error banner if provisioning failed on this step
    if let Some(step) = view.provision.steps.get(step_idx) {
        if let StepStatus::Failed(ref err) = step.status {
            lines.push(Line::styled(
                " ERROR:",
                Style::default().fg(Color::Red),
            ));
            let err_width = inner.width.saturating_sub(2) as usize;
            let err_text = err.trim();
            for chunk in err_text.as_bytes().chunks(err_width.max(1)) {
                let s = String::from_utf8_lossy(chunk);
                lines.push(Line::styled(
                    format!(" {s}"),
                    Style::default().fg(Color::Red),
                ));
            }
            lines.push(Line::raw(""));
        }
    }

    let step_logs = view.provision.step_logs.get(step_idx);
    let log_lines = step_logs.map(|l| l.as_slice()).unwrap_or(&[]);

    if log_lines.is_empty() {
        let is_running = view
            .provision
            .steps
            .get(step_idx)
            .map(|s| s.status == StepStatus::Running)
            .unwrap_or(false);

        if is_running {
            lines.push(Line::from(vec![Span::styled(
                format!(" {} Waiting for output...", SPINNER[spin]),
                Style::default().fg(Color::DarkGray),
            )]));
        } else if view.provision.is_done() {
            lines.push(Line::styled(
                " All steps completed.",
                Style::default().fg(Color::Green),
            ));
        } else {
            lines.push(Line::styled(
                " No output yet.",
                Style::default().fg(Color::DarkGray),
            ));
        }
    } else {
        let used = lines.len() as u16;
        let max_lines = inner.height.saturating_sub(used) as usize;
        let log_width = inner.width.saturating_sub(2) as usize;
        let start = log_lines.len().saturating_sub(max_lines);
        for log_line in &log_lines[start..] {
            let trimmed = log_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let display = if trimmed.len() > log_width {
                format!(" {}…", &trimmed[..log_width.saturating_sub(2)])
            } else {
                format!(" {trimmed}")
            };
            lines.push(Line::styled(display, Style::default().fg(Color::DarkGray)));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Snapshots Tab ───────────────────────────────────────────────────────

fn draw_snapshots_tab(f: &mut Frame, area: Rect, ss: &SnapshotsState, spin: usize) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);

    draw_snapshots_list(f, chunks[0], ss, spin);
    draw_snapshot_detail(f, chunks[1], ss);
}

fn draw_snapshots_list(f: &mut Frame, area: Rect, ss: &SnapshotsState, spin: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Snapshots ")
        .border_style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    if ss.loading && ss.list.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[spin]),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("Loading..."),
        ]));
    }

    for (i, snap) in ss.list.iter().enumerate() {
        let is_selected = i == ss.selected;
        let name_style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };

        let monthly = snapshot_monthly_cost(snap.size_gigabytes);
        let cost_text = format!(" ({:.1} GB, ~${:.2}/mo)", snap.size_gigabytes, monthly);
        lines.push(Line::from(vec![
            Span::styled(" ● ", Style::default().fg(Color::Cyan)),
            Span::styled(&snap.name, name_style),
            Span::styled(cost_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Show pending snapshots with spinner
    for name in &ss.pending {
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", SPINNER[spin]), Style::default().fg(Color::Yellow)),
            Span::styled(name, Style::default().fg(Color::Yellow)),
            Span::styled(" (creating...)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    if !ss.loading && ss.list.is_empty() && ss.pending.is_empty() {
        lines.push(Line::styled(
            " No snapshots",
            Style::default().fg(Color::DarkGray),
        ));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_snapshot_detail(f: &mut Frame, area: Rect, ss: &SnapshotsState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ")
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let snap = match ss.list.get(ss.selected) {
        Some(s) => s,
        None => {
            f.render_widget(
                Paragraph::new(Line::styled(
                    " No snapshot selected",
                    Style::default().fg(Color::DarkGray),
                )),
                inner,
            );
            return;
        }
    };

    let ago = time_ago(&snap.created_at);
    let regions = if snap.regions.is_empty() {
        "—".to_string()
    } else {
        snap.regions.join(", ")
    };
    let monthly = snapshot_monthly_cost(snap.size_gigabytes);

    let lines = vec![
        Line::from(vec![
            Span::styled(" Name:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(&snap.name, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" ID:      ", Style::default().fg(Color::DarkGray)),
            Span::raw(snap.id.to_string()),
        ]),
        Line::from(vec![
            Span::styled(" Created: ", Style::default().fg(Color::DarkGray)),
            Span::raw(if ago.is_empty() { snap.created_at.clone() } else { ago }),
        ]),
        Line::from(vec![
            Span::styled(" Size:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:.2} GB", snap.size_gigabytes)),
        ]),
        Line::from(vec![
            Span::styled(" Cost:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("~${:.2}/mo", monthly), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled(" Regions: ", Style::default().fg(Color::DarkGray)),
            Span::raw(regions),
        ]),
    ];

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

    let (icon, icon_color, status_text) = match info.status {
        KeyStatus::Unknown => ("?", Color::DarkGray, "Unknown"),
        KeyStatus::Checking => (SPINNER[spin], Color::Yellow, "Checking..."),
        KeyStatus::Ok => ("✓", Color::Green, "Ok"),
        KeyStatus::Error => ("✗", Color::Red, "Error"),
    };
    lines.push(Line::from(vec![
        Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(status_text, Style::default().fg(icon_color)),
    ]));

    if let Some(ref msg) = info.message {
        lines.push(Line::styled(
            format!("    {msg}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.push(Line::raw(""));

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

    let bindings: Vec<(&str, &str)> = match state.tab {
        Tab::Droplets => match state.droplets.focus {
            DFocus::List => vec![
                ("↑↓", "navigate"),
                ("Enter/→", "select"),
                ("D", "delete"),
                ("Tab", "snapshots"),
                ("q", "quit"),
            ],
            DFocus::DetailInfo => vec![
                ("↑↓", "navigate"),
                ("Enter", "action"),
                ("→", "provisioning"),
                ("D", "delete"),
                ("←/Esc", "back"),
                ("q", "quit"),
            ],
            DFocus::DetailProvision => {
                let mut b = vec![("↑↓", "select step")];
                // Show restart hint if selected step has failed
                let has_failure = state
                    .droplets
                    .registry
                    .get_by_index(state.droplets.selected)
                    .and_then(|v| v.provision.steps.get(state.droplets.provision_selected))
                    .map(|s| matches!(s.status, StepStatus::Failed(_)))
                    .unwrap_or(false);
                if has_failure {
                    b.push(("R", "restart"));
                }
                b.extend_from_slice(&[("←", "back"), ("D", "delete"), ("q", "quit")]);
                b
            }
        },
        Tab::Snapshots => vec![
            ("↑↓", "navigate"),
            ("R", "rename"),
            ("D", "delete"),
            ("Tab", "config"),
            ("q", "quit"),
        ],
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
