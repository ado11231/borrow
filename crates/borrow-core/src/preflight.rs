//! The checks that stand in for an installer.
//!
//! For everyone who is not the author, these checks are the setup experience. Every
//! failure prints the command that fixes it and nothing is installed automatically.
//! Successes print a line too, so a working box visibly passes.

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

/// One line of the report.
#[derive(Debug)]
pub struct Check {
    pub label: String,
    pub state: State,
    pub fix: Option<String>,
}

impl Check {
    pub fn pass(label: impl Into<String>) -> Check {
        Check { label: label.into(), state: State::Pass, fix: None }
    }

    pub fn warn(label: impl Into<String>, fix: impl Into<String>) -> Check {
        Check { label: label.into(), state: State::Warn, fix: Some(fix.into()) }
    }

    pub fn fail(label: impl Into<String>, fix: impl Into<String>) -> Check {
        Check { label: label.into(), state: State::Fail, fix: Some(fix.into()) }
    }
}

/// Print the report and say whether anything is broken. The caller decides what to
/// do about a failure, because `serve` should stop and `link` may want to carry on.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        let mark = match check.state {
            State::Pass => "✓",
            State::Warn => "!",
            State::Fail => "✗",
        };

        match &check.fix {
            Some(fix) => eprintln!("{mark} {:<34}  →  {fix}", check.label),
            None => eprintln!("{mark} {}", check.label),
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

/// Whether an ssh server is accepting connections on this machine. Phase 2 mounts
/// the Client's files onto the Agent, so both machines end up needing one.
pub fn ssh_server_check() -> Check {
    let addr: SocketAddr = ([127, 0, 0, 1], 22).into();

    if is_listening(addr) {
        return Check::pass("ssh server running");
    }

    let fix = if cfg!(target_os = "macos") {
        "System Settings, General, Sharing, Remote Login: ON".to_string()
    } else {
        "sudo systemctl enable --now sshd".to_string()
    };

    Check::fail("ssh server not running", fix)
}

/// A program that must be present, with the install line for this machine.
pub fn tool_check(program: &str, state_when_missing: State) -> Check {
    if is_installed(program) {
        return Check::pass(format!("{program} present"));
    }

    let label = format!("{program} not installed");
    let fix = install_hint(program);

    match state_when_missing {
        State::Warn => Check::warn(label, fix),
        _ => Check::fail(label, fix),
    }
}

/// Whether this account can create things in a directory borrow owns.
///
/// Both of borrow's directories live under root owned parents, so a stranger's
/// first `serve` would otherwise fail deep inside a shell script with a bare
/// permission error. Checked here instead, with the two commands that fix it.
pub fn writable_check(path: &str, purpose: &str) -> Check {
    let dir = std::path::Path::new(path);

    let fix = format!("sudo mkdir -p {path} && sudo chown $(id -un) {path}");

    if !dir.exists() {
        return Check::fail(format!("{path} does not exist ({purpose})"), fix);
    }

    let probe = dir.join(".borrow-write-test");

    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Check::pass(format!("{path} is writable"))
        }
        Err(_) => Check::fail(format!("{path} is not writable ({purpose})"), fix),
    }
}

/// The checks `borrow serve` runs before it listens. sshfs is required from Phase 2
/// onward, because without it there is no mount and the box can only run commands
/// against files it cannot see. rsync stays a warning; nothing needs it yet.
pub fn serve_checks() -> Vec<Check> {
    let mut checks = vec![
        ssh_server_check(),
        tool_check("sshfs", State::Fail),
        tool_check("rsync", State::Warn),
        writable_check(crate::mount::MOUNT_BASE, "where your files appear"),
        writable_check(crate::mount::ARTIFACT_BASE, "where build output goes"),
    ];

    checks.push(match is_installed("nvidia-smi") {
        true => Check::pass("gpu tooling present"),
        false => Check::warn("no nvidia-smi, gpu reporting off", "fine if the box has no Nvidia gpu"),
    });

    checks
}
