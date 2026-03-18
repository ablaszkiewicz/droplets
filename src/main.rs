mod app;
mod backend;
mod types;
mod ui;

use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{App, Msg};

const TICK_RATE: Duration = Duration::from_millis(100);

fn main() -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut app = App::new();

    // Start initial check
    app.start_initial_check(&tx);

    let mut last_tick = Instant::now();

    loop {
        // Render
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll for events
        let timeout = TICK_RATE.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release/repeat)
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key, &tx);
                }
            }
        }

        // Process background messages
        while let Ok(msg) = rx.try_recv() {
            app.handle_message(msg, &tx);
        }

        // Tick
        if last_tick.elapsed() >= TICK_RATE {
            app.tick(&tx);
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    app.cleanup();
    Ok(())
}
