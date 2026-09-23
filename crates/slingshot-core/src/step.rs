//! A step that takes a moment, such as connecting or syncing. A spinner shows while it
//! runs, then one line says how it went and how long it took. The spinner only draws on an
//! interactive terminal, so piped output gets just the finished line.

use crate::presentation::{Style, Tone, dumb_terminal};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::time::{Duration, Instant};

/// Finished lines pad their text to this width so the times line up.
const TEXT_WIDTH: usize = 38;

const INSTANT: Duration = Duration::from_millis(100);

pub struct Step {
    bar: Option<ProgressBar>,
    started: Instant,
}

/// Begin a step described by `text`, such as `Connecting to archbox`.
pub fn start(text: impl Into<String>) -> Step {
    let animate = std::io::stderr().is_terminal() && !dumb_terminal();
    Step {
        bar: animate.then(|| spinner(text.into())),
        started: Instant::now(),
    }
}

fn spinner(text: String) -> ProgressBar {
    let template = match Style::stderr().colored() {
        true => "{spinner:.cyan} {msg}",
        false => "{spinner} {msg}",
    };
    let bar = ProgressBar::new_spinner().with_message(text);
    bar.set_style(
        ProgressStyle::with_template(template)
            .expect("spinner template is valid")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
    );
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

impl Step {
    /// Replace the text while the step keeps running.
    pub fn set(&self, text: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.set_message(text.into());
        }
    }

    /// Print a line without the spinner drawing over it.
    pub fn println(&self, line: impl std::fmt::Display) {
        match &self.bar {
            Some(bar) => bar.suspend(|| eprintln!("{line}")),
            None => eprintln!("{line}"),
        }
    }

    pub fn done(self, text: impl std::fmt::Display) {
        self.finish(text, Tone::Good);
    }

    pub fn warn(self, text: impl std::fmt::Display) {
        self.finish(text, Tone::Warning);
    }

    /// Remove the spinner and print nothing, for a step whose error is reported elsewhere.
    pub fn clear(self) {}

    /// An instant step gets no time, because `0.0s` says nothing.
    fn finish(mut self, text: impl std::fmt::Display, tone: Tone) {
        self.hide();
        let style = Style::stderr();
        let took = self.started.elapsed();
        match took < INSTANT {
            true => eprintln!("{}", style.status(text, tone)),
            false => eprintln!(
                "{} {}",
                style.status(pad(&text.to_string()), tone),
                style.dim(elapsed(took))
            ),
        }
    }

    fn hide(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

impl Drop for Step {
    fn drop(&mut self) {
        self.hide();
    }
}

fn pad(text: &str) -> String {
    format!("{text:<TEXT_WIDTH$}")
}

/// `0.4s`, `12s`, or `2m 5s`: precise when short, rounded once it no longer matters.
pub fn elapsed(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    match duration.as_secs() {
        0..10 => format!("{seconds:.1}s"),
        10..60 => format!("{}s", duration.as_secs()),
        whole => format!("{}m {}s", whole / 60, whole % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_gets_coarser_as_it_grows() {
        assert_eq!(elapsed(Duration::from_millis(420)), "0.4s");
        assert_eq!(elapsed(Duration::from_millis(3_540)), "3.5s");
        assert_eq!(elapsed(Duration::from_millis(12_900)), "12s");
        assert_eq!(elapsed(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn finished_text_is_padded_so_times_line_up() {
        assert_eq!(pad("Synced 3 files").len(), TEXT_WIDTH);
        assert_eq!(pad(&"x".repeat(50)).len(), 50);
    }
}
