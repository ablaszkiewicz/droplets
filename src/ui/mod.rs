mod main_view;
mod popup;
mod welcome;

use ratatui::Frame;

use crate::app::{App, Screen};

pub fn draw(f: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Welcome(state) => welcome::draw(f, state, app.spinner_idx),
        Screen::Main(state) => {
            // CreateDroplet takes over the whole screen
            if let Some(crate::app::Popup::CreateDroplet(cs)) = &state.popup {
                popup::draw_create_fullscreen(f, cs, app.spinner_idx);
            } else {
                main_view::draw(f, state, app.spinner_idx, &app.notification);
                if let Some(popup) = &state.popup {
                    popup::draw(f, popup, app.spinner_idx);
                }
            }
        }
    }
}
