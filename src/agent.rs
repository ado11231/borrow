//! `borrow serve`: the daemon that runs on the Agent.
//!
//! It answers questions about the box and hands out exactly one key at pairing time.
//! It never runs your work. Commands travel over ssh instead, which is why this file
//! stays small and has nothing resembling a shell in it.

use crate::keys::marker;
use crate::preflight;
use crate::protocol::{Paired, Request, Response};
use crate::telemetry;
use anyhow::Context;
use directories::BaseDirs;
use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

/// How long a pairing code stays good for. Long enough to walk to the other machine,
/// short enough that a code left on screen overnight is worthless.
const CODE_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// Characters a pairing code is built from. No I, O, 0 or 1, because somebody is
/// going to read this off one screen and type it into another.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

const CODE_LENGTH: usize = 8;

/// The one time token that lets a Client install its key. Single use: pairing
/// consumes it, and there is no way to ask the daemon what it was.
struct Pairing {
    token: String,
    expires: Instant,
}

/// Everything the daemon needs while it runs. The name is chosen here and travels
/// to the Client at pairing, so the "running on" line never has to guess.
struct Agent {
    name: String,
    user: String,
    pairing: Mutex<Option<Pairing>>,
}

/// Start the daemon: run the checks, print a pairing code, then listen.
pub async fn serve(name: Option<String>, port: u16) -> anyhow::Result<i32> {
    let name = name
        .or_else(sysinfo::System::host_name)
        .unwrap_or_else(|| "agent".to_string());

    if preflight::report(&preflight::serve_checks()) {
        anyhow::bail!("fix the lines marked ✗ above, then run borrow serve again");
    }

    let user = whoami().context("could not work out which user is running the daemon")?;
    let token = new_token();

    let agent = Arc::new(Agent {
        name: name.clone(),
        user,
        pairing: Mutex::new(Some(Pairing {
            token: token.clone(),
            expires: Instant::now() + CODE_LIFETIME,
        })),
    });

    let addresses = bind_addresses(port);
    announce(&name, &addresses, &token);

    let mut listeners = Vec::new();
    for addr in &addresses {
        match TcpListener::bind(addr).await {
            Ok(listener) => listeners.push(listener),
            Err(e) => warn!("could not listen on {addr}: {e}"),
        }
    }

    if listeners.is_empty() {
        anyhow::bail!("could not listen on any address; is port {port} already in use?");
    }

    let mut tasks = Vec::new();
    for listener in listeners {
        let agent = Arc::clone(&agent);
        tasks.push(tokio::spawn(accept_loop(listener, agent)));
    }

    tokio::signal::ctrl_c().await.ok();
    eprintln!("\nstopping");

    Ok(0)
}

/// Take connections forever, one task each, so a slow Client never blocks another.
async fn accept_loop(listener: TcpListener, agent: Arc<Agent>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let agent = Arc::clone(&agent);
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, agent).await {
                        warn!("request from {peer} failed: {e:#}");
                    }
                });
            }
            Err(e) => warn!("could not accept a connection: {e}"),
        }
    }
}

/// Read one request, write one response. Anything that goes wrong comes back as an
/// Error response rather than a dropped connection, so the Client can explain it.
async fn handle(mut stream: TcpStream, agent: Arc<Agent>) -> anyhow::Result<()> {
    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line).await?;

    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => answer(request, &agent),
        Err(e) => Response::Error { message: format!("could not understand that request: {e}") },
    };

    let mut reply = serde_json::to_string(&response)?;
    reply.push('\n');
    stream.write_all(reply.as_bytes()).await?;

    Ok(())
}

fn answer(request: Request, agent: &Agent) -> Response {
    match request {
        Request::Info => Response::Info(telemetry::specs(&agent.name)),
        Request::Health => Response::Health(telemetry::health()),
        Request::Pair { token, client, public_key } => pair(agent, &token, &client, &public_key),
    }
}

/// Trade a valid token for an installed key. The token is taken out of the daemon
/// before the key is written, so a second attempt with the same code finds nothing
/// even if two Clients race each other.
fn pair(agent: &Agent, token: &str, client: &str, public_key: &str) -> Response {
    let claimed = {
        let mut slot = agent.pairing.lock().expect("pairing lock was poisoned");

        match slot.as_ref() {
            None => None,
            Some(p) if Instant::now() > p.expires => {
                *slot = None;
                None
            }
            Some(p) if p.token != token => Some(false),
            Some(_) => {
                *slot = None;
                Some(true)
            }
        }
    };

    match claimed {
        None => Response::Error {
            message: "that pairing code has expired or was already used; run borrow serve again for a fresh one".to_string(),
        },
        Some(false) => Response::Error { message: "that pairing code is not right".to_string() },
        Some(true) => match install_key(client, public_key) {
            Ok(path) => {
                info!("paired with {client}");
                eprintln!("✓ paired with {client}, key added to {}", path.display());

                Response::Paired(Paired {
                    name: agent.name.clone(),
                    user: agent.user.clone(),
                    host_keys: host_keys(),
                    specs: telemetry::specs(&agent.name),
                })
            }
            Err(e) => Response::Error { message: format!("could not install the key: {e:#}") },
        },
    }
}

/// Add the Client's public key to authorized_keys, replacing any older key from the
/// same Client. The file is read, changed in memory, and written back whole, because
/// appending to a file that has no trailing newline joins two keys into one broken
/// line and silently locks you out.
fn install_key(client: &str, public_key: &str) -> anyhow::Result<PathBuf> {
    let key = public_key.trim();

    if !key.starts_with("ssh-") && !key.starts_with("ecdsa-") {
        anyhow::bail!("that does not look like an ssh public key");
    }

    let Some(base) = BaseDirs::new() else {
        anyhow::bail!("could not determine home directory");
    };

    let dir = base.home_dir().join(".ssh");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    set_mode(&dir, 0o700)?;

    let file = dir.join("authorized_keys");
    let existing = fs::read_to_string(&file).unwrap_or_default();
    let tag = marker(client);

    let mut lines: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.ends_with(&tag))
        .collect();

    lines.push(key);

    let mut body = lines.join("\n");
    body.push('\n');

    let mut handle = fs::File::create(&file)
        .with_context(|| format!("could not write {}", file.display()))?;
    handle.write_all(body.as_bytes())?;
    set_mode(&file, 0o600)?;

    Ok(file)
}

/// Lock a file down to its owner. ssh refuses to use keys and authorized_keys files
/// that anybody else can read, so this is required rather than tidy.
fn set_mode(path: &std::path::Path, mode: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("could not set permissions on {}", path.display()))?;
    }

    let _ = (path, mode);
    Ok(())
}

/// This box's ssh host keys, read from where sshd publishes them. Sending these at
/// pairing is what lets the Client trust the box on its very first connection,
/// instead of asking somebody to compare a fingerprint by eye.
fn host_keys() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/etc/ssh") else {
        return Vec::new();
    };

    let mut keys: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            name.starts_with("ssh_host_") && name.ends_with("_key.pub")
        })
        .filter_map(|path| fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    keys.sort();
    keys
}

/// A fresh pairing code. Uses the operating system's randomness, so a code cannot be
/// guessed from knowing when the daemon started.
fn new_token() -> String {
    (0..CODE_LENGTH)
        .map(|_| CODE_ALPHABET[rand::random_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

/// The account the daemon is running as, which is who the installed key will log in as.
fn whoami() -> anyhow::Result<String> {
    let out = std::process::Command::new("id").arg("-un").output()?;

    if !out.status.success() {
        anyhow::bail!("id -un failed");
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Loopback plus whichever local networks this machine is actually on. Never
/// 0.0.0.0, so the daemon is not offered to an interface nobody asked about.
fn bind_addresses(port: u16) -> Vec<SocketAddr> {
    let mut addresses = vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))];

    for probe in ["8.8.8.8:53", "100.100.100.100:53"] {
        if let Some(ip) = local_ip_towards(probe) {
            let addr = SocketAddr::new(ip, port);
            if !addresses.contains(&addr) {
                addresses.push(addr);
            }
        }
    }

    addresses
}

/// Which of this machine's addresses would be used to reach somewhere else. UDP
/// connect only picks a route, so nothing is sent and the target is never contacted.
fn local_ip_towards(target: &str) -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(target).ok()?;

    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if ip.is_loopback() || ip.is_unspecified() => None,
        ip => Some(ip),
    }
}

/// Print the pairing code once, to the owner's own console. It is never written to
/// a file and never logged, because whoever holds it can install a key.
fn announce(name: &str, addresses: &[SocketAddr], token: &str) {
    let reachable: Vec<&SocketAddr> = addresses.iter().filter(|a| !a.ip().is_loopback()).collect();
    let best = reachable.first().copied().or_else(|| addresses.first());

    eprintln!();
    eprintln!("borrow is serving {name}");

    match best {
        Some(addr) => {
            eprintln!();
            eprintln!("  on the other machine, run:");
            eprintln!();
            eprintln!("      borrow link {}:{}:{token}", addr.ip(), addr.port());
            eprintln!();
        }
        None => eprintln!("  no address to pair on"),
    }

    for addr in reachable.iter().skip(1) {
        eprintln!("  also reachable at {}:{}", addr.ip(), addr.port());
    }

    eprintln!("  the code works once and expires in {} minutes", CODE_LIFETIME.as_secs() / 60);
    eprintln!("  ctrl-c to stop");
    eprintln!();
}
