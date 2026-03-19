use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{WelcomePhase, WelcomeState};
use crate::types::SPINNER;

pub fn draw(f: &mut Frame, state: &WelcomeState, spin: usize) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Center a box
    let box_w = 60u16.min(area.width.saturating_sub(4));
    let box_h = 18u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(box_w)) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;
    let rect = Rect::new(x, y, box_w, box_h);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Droplets Setup ")
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = Vec::new();
    let mut footer_lines: Vec<Line> = Vec::new();

    // Show completed github check if we're past it
    if let Some(msg) = &state.github_done_msg {
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::raw(msg.as_str()),
        ]));
        lines.push(Line::raw(""));
    }

    match &state.phase {
        WelcomePhase::CheckingGithub => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Checking GitHub SSH key..."),
            ]));
            footer_lines.push(Line::styled(
                "  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::GithubOk(msg) => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(msg.as_str()),
            ]));
            footer_lines.push(Line::styled(
                "  Enter continue",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::GithubMissing => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("No GitHub SSH key found"),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::raw("  A key will be generated for you."));
            footer_lines.push(Line::styled(
                "  Enter generate  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::GeneratingGithub => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Generating SSH key..."),
            ]));
        }

        WelcomePhase::GithubGenerated { copied, .. } => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw("SSH key generated!"),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::raw("  Add this key to GitHub:"));
            lines.push(Line::styled(
                "  https://github.com/settings/ssh/new",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            lines.push(Line::raw(""));

            if *copied {
                lines.push(Line::from(vec![
                    Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                    Span::raw("Copied to clipboard"),
                ]));
            } else {
                lines.push(Line::styled(
                    "  [C] Copy public key to clipboard",
                    Style::default().fg(Color::White),
                ));
            }

            footer_lines.push(Line::styled(
                "  Enter test connection  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::TestingGithub { .. } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Testing GitHub SSH connection..."),
            ]));
            footer_lines.push(Line::styled(
                "  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::GithubFailed {
            error,
            public_key,
            copied,
        } => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("GitHub SSH test failed"),
            ]));
            lines.push(Line::styled(
                format!("    {error}"),
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::raw(""));

            if public_key.is_some() {
                if *copied {
                    lines.push(Line::from(vec![
                        Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                        Span::raw("Copied to clipboard"),
                    ]));
                } else {
                    lines.push(Line::styled(
                        "  [C] Copy public key to clipboard",
                        Style::default().fg(Color::White),
                    ));
                }
            }

            footer_lines.push(Line::styled(
                "  Enter retest  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::CheckingDo => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Checking DigitalOcean API key..."),
            ]));
            footer_lines.push(Line::styled(
                "  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::DoOk(msg) => {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(msg.as_str()),
            ]));
            footer_lines.push(Line::styled(
                "  Enter continue",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::DoMissing => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("No DigitalOcean API key found"),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::raw("  Create one at:"));
            lines.push(Line::styled(
                "  cloud.digitalocean.com/account/api/tokens",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            lines.push(Line::raw(""));
            lines.push(Line::raw("  Required scopes:"));
            lines.push(Line::styled(
                "  account:read, droplet:create/read/delete",
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::styled(
                "  image:create, snapshot:read/delete, ssh_key:create/read",
                Style::default().fg(Color::DarkGray),
            ));
            footer_lines.push(Line::styled(
                "  Enter set up  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::DoInput(input) => {
            lines.push(Line::raw("  Enter your DigitalOcean API token:"));
            lines.push(Line::raw(""));

            let display: String = input.display();
            let cursor_x = inner.x + 4 + input.cursor as u16;
            lines.push(Line::from(vec![
                Span::raw("  > "),
                Span::styled(display, Style::default().fg(Color::White)),
            ]));

            let cursor_y = inner.y + lines.len() as u16 - 1;
            if cursor_x < inner.x + inner.width && cursor_y < inner.y + inner.height {
                f.set_cursor_position((cursor_x, cursor_y));
            }

            footer_lines.push(Line::styled(
                "  Enter save  Esc cancel",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::TestingDo => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", SPINNER[spin]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("Testing DigitalOcean API key..."),
            ]));
            footer_lines.push(Line::styled(
                "  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }

        WelcomePhase::DoFailed(error) => {
            lines.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                Span::raw("DigitalOcean API test failed"),
            ]));
            lines.push(Line::styled(
                format!("    {error}"),
                Style::default().fg(Color::DarkGray),
            ));
            footer_lines.push(Line::styled(
                "  Enter try again  Esc skip",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    // Render content
    let content_height = inner.height.saturating_sub(footer_lines.len() as u16 + 1);
    let content_area = Rect::new(inner.x, inner.y, inner.width, content_height);
    f.render_widget(Paragraph::new(lines), content_area);

    // Render footer at bottom of inner area
    if !footer_lines.is_empty() {
        let footer_y = inner.y + inner.height - footer_lines.len() as u16;
        let footer_area = Rect::new(inner.x, footer_y, inner.width, footer_lines.len() as u16);
        f.render_widget(Paragraph::new(footer_lines), footer_area);
    }
}
