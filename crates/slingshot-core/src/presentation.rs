//! Shared formatting for Slingshot output. Stream detection keeps redirected output plain.

use std::io::{IsTerminal, stderr, stdout};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn enabled(self, terminal: bool, no_color: bool, dumb: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => terminal && !no_color && !dumb,
        }
    }
}

static COLOR: OnceLock<ColorMode> = OnceLock::new();

pub fn configure(mode: ColorMode) {
    let _ = COLOR.set(mode);
}

#[derive(Clone, Copy)]
pub enum Tone {
    Good,
    Warning,
    Error,
    Info,
}

#[derive(Clone, Copy)]
pub struct Style {
    color: bool,
}

impl Style {
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    pub fn stdout() -> Self {
        Self::for_terminal(stdout().is_terminal())
    }

    pub fn stderr() -> Self {
        Self::for_terminal(stderr().is_terminal())
    }

    fn for_terminal(terminal: bool) -> Self {
        let mode = COLOR.get().copied().unwrap_or_default();
        Self::new(mode.enabled(terminal, no_color(), dumb_terminal()))
    }

    pub fn paint(self, text: impl std::fmt::Display, tone: Tone) -> String {
        if !self.color {
            return text.to_string();
        }
        let code = match tone {
            Tone::Good => 32,
            Tone::Warning => 33,
            Tone::Error => 31,
            Tone::Info => 36,
        };
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    pub fn colored(self) -> bool {
        self.color
    }

    pub fn dim(self, text: impl std::fmt::Display) -> String {
        match self.color {
            true => format!("\x1b[2m{text}\x1b[0m"),
            false => text.to_string(),
        }
    }

    pub fn heading(self, text: impl std::fmt::Display) -> String {
        self.paint(text, Tone::Info)
    }

    pub fn status(self, text: impl std::fmt::Display, tone: Tone) -> String {
        let label = match tone {
            Tone::Good => "✓",
            Tone::Warning => "!",
            Tone::Error => "✗",
            Tone::Info => "▶",
        };
        format!("{} {text}", self.paint(label, tone))
    }
}

/// `1 file` or `3 files`: the plural form English needs for a count in a sentence.
pub fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

/// One indented `label   value` line. Colourless on purpose: the label is structure, and
/// any emphasis belongs to the value the caller passes in.
pub fn row(label: &str, value: impl std::fmt::Display) -> String {
    format!("  {label:<12} {value}\n")
}

/// Whether the NO_COLOR convention is in effect.
pub fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

pub fn dumb_terminal() -> bool {
    std::env::var_os("TERM").is_some_and(|value| value == "dumb")
}

pub fn success(text: impl std::fmt::Display) {
    eprintln!("{}", Style::stderr().status(text, Tone::Good));
}

pub fn warning(text: impl std::fmt::Display) {
    eprintln!("{}", Style::stderr().status(text, Tone::Warning));
}

pub fn progress(text: impl std::fmt::Display) {
    eprintln!("{}", Style::stderr().status(text, Tone::Info));
}

pub fn detail(label: &str, value: impl std::fmt::Display) {
    eprint!("{}", row(label, value));
}

/// A byte count in MiB, shown as MiB or GiB depending on size.
pub fn capacity(mib: u64) -> String {
    if mib < 1024 {
        format!("{:.1} MiB", mib as f64)
    } else {
        format!("{:.1} GiB", mib as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_preserves_small_values_and_fractional_gib() {
        assert_eq!(capacity(0), "0.0 MiB");
        assert_eq!(capacity(512), "512.0 MiB");
        assert_eq!(capacity(1024), "1.0 GiB");
        assert_eq!(capacity(1536), "1.5 GiB");
    }

    #[test]
    fn automatic_color_requires_a_suitable_terminal() {
        assert!(ColorMode::Auto.enabled(true, false, false));
        assert!(!ColorMode::Auto.enabled(false, false, false));
        assert!(!ColorMode::Auto.enabled(true, true, false));
        assert!(!ColorMode::Auto.enabled(true, false, true));
    }

    #[test]
    fn explicit_modes_override_environment_and_stream() {
        assert!(ColorMode::Always.enabled(false, true, true));
        assert!(!ColorMode::Never.enabled(true, false, false));
    }

    #[test]
    fn plain_status_remains_readable_without_color() {
        assert_eq!(
            Style::new(false).status("Low memory", Tone::Warning),
            "! Low memory"
        );
        assert_eq!(
            Style::new(true).paint("Busy", Tone::Warning),
            "\x1b[33mBusy\x1b[0m"
        );
    }
}
