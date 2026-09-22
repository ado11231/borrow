//! Full screen views that refresh every two seconds until Q or Ctrl C.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, queue, terminal};
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(2);

/// Puts the terminal back exactly as it was, including after errors and panics.
struct Screen;

impl Screen {
    fn enter() -> anyhow::Result<Screen> {
        terminal::enable_raw_mode()?;
        let screen = Screen;
        execute!(
            std::io::stdout(),
            terminal::EnterAlternateScreen,
            cursor::Hide
        )?;
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            cursor::Show,
            terminal::LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn require_terminal() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "Live views need an interactive terminal. Use borrow health for a single snapshot"
    );
    Ok(())
}

/// Draw `frame` output repeatedly. `frame` returns the text to show; an error is shown
/// in place so a brief network problem does not end the view.
pub async fn show<F, Fut>(mut frame: F) -> anyhow::Result<i32>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<String>>,
{
    require_terminal()?;
    let _screen = Screen::enter()?;
    loop {
        let started = Instant::now();
        let body = match frame().await {
            Ok(body) => body,
            Err(error) => format!("\nCould not refresh: {error:#}\n"),
        };
        draw(&body)?;
        let wait = REFRESH.saturating_sub(started.elapsed());
        if tokio::task::spawn_blocking(move || quit_requested(wait)).await?? {
            return Ok(0);
        }
    }
}

fn draw(body: &str) -> anyhow::Result<()> {
    let mut out = std::io::stdout();
    queue!(
        out,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    for line in body.lines() {
        write!(out, "{line}\r\n")?;
    }
    write!(out, "\r\n  Refreshing every 2 seconds. Press Q to quit\r\n")?;
    out.flush()?;
    Ok(())
}

fn quit_requested(wait: Duration) -> anyhow::Result<bool> {
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !event::poll(remaining)? {
            return Ok(false);
        }
        if let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
            && is_quit(key.code, key.modifiers)
        {
            return Ok(true);
        }
    }
}

fn is_quit(code: KeyCode, modifiers: KeyModifiers) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => true,
        KeyCode::Char('c') | KeyCode::Char('C') => modifiers.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_escape_and_ctrl_c_quit() {
        assert!(is_quit(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(is_quit(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        assert!(is_quit(KeyCode::Esc, KeyModifiers::NONE));
        assert!(is_quit(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!is_quit(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!is_quit(KeyCode::Enter, KeyModifiers::NONE));
    }
}
