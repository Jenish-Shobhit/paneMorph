//! The small "paneMorph" notice popup shown when toasts are off (7.6).
//! It closes on any key or after four seconds.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::ui::view::{ACCENT, FG, MUTE};

pub const LIFETIME: Duration = Duration::from_secs(4);

pub fn draw(frame: &mut Frame, message: &str) {
    let area = frame.area();
    if area.width < 4 || area.height < 2 {
        return;
    }
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let text = vec![
        Line::from(Span::styled(
            "paneMorph",
            ratatui::style::Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            message.to_string(),
            ratatui::style::Style::new().fg(FG),
        )),
    ];
    let body = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), body);
    let footer = Line::from(Span::styled(
        "any key closes",
        ratatui::style::Style::new().fg(MUTE),
    ));
    frame
        .buffer_mut()
        .set_line(inner.x, area.y + area.height - 1, &footer, inner.width);
}

pub fn run() -> i32 {
    let message = std::env::var("PANEMORPH_NOTICE").unwrap_or_else(|_| "paneMorph".into());
    let result = (|| -> io::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let deadline = Instant::now() + LIFETIME;
        loop {
            terminal.draw(|frame| draw(frame, &message))?;
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            if event::poll(left.min(Duration::from_millis(250)))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => return Ok(()),
                    Event::Mouse(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    })();
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    i32::from(result.is_err())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn edge_7_6_notice_shows_title_message_and_hint() {
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| draw(f, "Already the last tab")).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(row(0).contains("paneMorph"));
        assert!(row(1).contains("Already the last tab"));
        assert!(row(4).contains("any key closes"));
    }
}
