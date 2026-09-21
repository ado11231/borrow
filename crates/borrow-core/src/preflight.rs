//! Setup checks with instructions for fixing missing tools and permissions.

use crate::presentation::{Style, Tone};
use crate::telemetry::is_installed;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// How long to wait when testing whether something is listening.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// How a single check turned out. `Warn` means the box works today but something
/// later will want this, so it is worth saying without blocking anybody.
#[derive(Debug, PartialEq)]
pub enum State {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug)]
pub struct Check {
    pub label: String,
    pub state: State,
    pub fix: Option<String>,
}

impl Check {
    pub fn pass(label: impl Into<String>) -> Check {
        Check {
            label: label.into(),
            state: State::Pass,
            fix: None,
        }
    }

    pub fn warn(label: impl Into<String>, fix: impl Into<String>) -> Check {
        Check {
            label: label.into(),
            state: State::Warn,
            fix: Some(fix.into()),
        }
    }

    pub fn fail(label: impl Into<String>, fix: impl Into<String>) -> Check {
        Check {
            label: label.into(),
            state: State::Fail,
            fix: Some(fix.into()),
        }
    }
}

/// Print each check and return true if any check failed.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        let tone = match check.state {
            State::Pass => Tone::Good,
            State::Warn => Tone::Warning,
            State::Fail => Tone::Error,
        };
        eprintln!("{}", Style::stderr().status(&check.label, tone));
        if let Some(fix) = &check.fix {
            eprintln!("  Fix: {fix}");
        }
    }

    checks.iter().any(|c| c.state == State::Fail)
}

/// The install command for this machine, so the fix line names a package manager
/// that actually exists here. Falls back to naming the package on its own.
pub fn install_hint(package: &str) -> String {
    let managers = [
        ("pacman", format!("sudo pacman -S {package}")),
        ("apt", format!("sudo apt install {package}")),
        ("dnf", format!("sudo dnf install {package}")),
        ("zypper", format!("sudo zypper install {package}")),
        ("apk", format!("sudo apk add {package}")),
        ("brew", format!("brew install {package}")),
    ];

    managers
        .into_iter()
        .find(|(manager, _)| is_installed(manager))
        .map(|(_, command)| command)
        .unwrap_or_else(|| format!("install {package} with your package manager"))
}

/// True when something accepts a connection at this address right now. This asks
/// the machine what is true rather than asking a config file what was intended.
pub fn is_listening(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
}

/// Whether an ssh server is accepting connections on this machine. The Agent needs one,
/// because ssh carries every command, transfer, and control message. The Client does not.
fn ssh_server_check() -> Check {
    let addr: SocketAddr = ([127, 0, 0, 1], 22).into();

    if is_listening(addr) {
        return Check::pass("SSH server running");
    }

    let fix = if cfg!(target_os = "macos") {
        "System Settings, General, Sharing, Remote Login: ON".to_string()
    } else {
        "sudo systemctl enable --now sshd".to_string()
    };

    Check::fail("SSH server not running", fix)
}

/// A program the machine needs, with the install line for this machine. `needed_for`
/// explains what stops working without it; an empty note means nothing works without it,
/// which is what makes it a failure rather than a warning.
pub fn tool_check(program: &str, needed_for: Option<&str>) -> Check {
    if is_installed(program) {
        return Check::pass(format!("Tool available: {program}"));
    }

    let fix = install_hint(program);

    match needed_for {
        Some(purpose) => Check::warn(
            format!("Tool not installed: {program} (needed for {purpose})"),
            fix,
        ),
        None => Check::fail(format!("Tool not installed: {program}"), fix),
    }
}

/// The checks `borrow serve` runs before it listens. SSH carries all work and control,
/// rsync copies project source, and tmux is only needed once someone uses attach.
pub fn serve_checks() -> Vec<Check> {
    vec![
        ssh_server_check(),
        tool_check("rsync", None),
        tool_check("tmux", Some("borrow attach")),
        match is_installed("nvidia-smi") {
            true => Check::pass("GPU reporting available"),
            false => Check::warn(
                "GPU reporting unavailable",
                "Optional when no NVIDIA GPU is installed",
            ),
        },
    ]
}
