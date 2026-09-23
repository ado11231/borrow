//! The Agent daemon: pairing over TCP, plus the private control service used for
//! everything after pairing. Work itself runs through SSH.

pub mod awake;
pub mod clients;
pub mod jobs;
pub mod projects;
pub mod runner;
pub mod service;
pub mod tunnel;

use anyhow::Context;
use borrow_core::keys;
use borrow_core::network::Network;
use borrow_core::preflight;
use borrow_core::protocol::{Paired, Request, Response};
use borrow_core::telemetry;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

/// How long a pairing code stays good for. Long enough to walk to the other machine,
/// short enough that a code left on screen overnight is worthless.
const CODE_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// Characters a pairing code is built from. No I, O, 0 or 1, because somebody is
/// going to read this off one screen and type it into another.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

const CODE_LENGTH: usize = 8;

/// Pairing requests are small. Anything larger, or slower than this, is dropped.
const PAIRING_LIMIT: u64 = 64 * 1024;
const PAIRING_TIMEOUT: Duration = Duration::from_secs(10);

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
    addresses: Vec<String>,
    root: PathBuf,
    iroh: String,
    pairing: Mutex<Option<Pairing>>,
}

/// Start the daemon: run the checks, print a pairing code, then listen.
pub async fn serve(name: Option<String>, port: u16) -> anyhow::Result<i32> {
    let name = name
        .or_else(sysinfo::System::host_name)
        .unwrap_or_else(|| "agent".to_string());

    if preflight::report(&preflight::serve_checks()) {
        anyhow::bail!("Fix the reported errors, then run borrow serve again");
    }

    let _service = service::start(name.clone())?;
    let root = service::root()?;
    let identity = borrow_core::tunnel::identity(&root)?;
    let user = whoami().context("Could not work out which user is running the daemon")?;
    let token = new_token();
    let addresses = bind_addresses(port);

    let agent = Arc::new(Agent {
        name: name.clone(),
        user,
        addresses: addresses
            .iter()
            .filter(|addr| !addr.ip().is_loopback())
            .map(|addr| addr.ip().to_string())
            .collect(),
        root,
        iroh: identity.public().to_string(),
        pairing: Mutex::new(Some(Pairing {
            token: token.clone(),
            expires: Instant::now() + CODE_LIFETIME,
        })),
    });

    let endpoint = tunnel::start(identity, agent.root.clone()).await?;

    announce(&name, &addresses, &token);

    let _awake = match awake::hold() {
        Some(awake) => {
            borrow_core::presentation::success("Keeping this machine awake while serving");
            Some(awake)
        }
        None => {
            borrow_core::presentation::warning(
                "Could not stop this machine from sleeping. If it sleeps, other machines cannot reach it until it wakes",
            );
            None
        }
    };

    let mut listeners = Vec::new();
    for addr in &addresses {
        match TcpListener::bind(addr).await {
            Ok(listener) => listeners.push(listener),
            Err(e) => warn!("Could not listen on {addr}: {e}"),
        }
    }

    if listeners.is_empty() {
        anyhow::bail!("Could not listen on any address; is port {port} already in use?");
    }

    let mut tasks = Vec::new();
    for listener in listeners {
        let agent = Arc::clone(&agent);
        tasks.push(tokio::spawn(accept_loop(listener, agent)));
    }

    tokio::spawn(report_reach(endpoint.clone()));

    tokio::signal::ctrl_c().await.ok();
    eprintln!("\nStopping");
    endpoint.close().await;

    Ok(0)
}

/// Say once whether other networks can reach this box. The same network works either way.
async fn report_reach(endpoint: iroh::Endpoint) {
    match tunnel::online(&endpoint).await {
        true => borrow_core::presentation::success("Reachable from other networks through iroh"),
        false => borrow_core::presentation::warning(
            "No iroh relay answered, so other networks cannot reach this box yet. The same network still works, and Borrow keeps trying",
        ),
    }
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
            Err(e) => warn!("Could not accept a connection: {e}"),
        }
    }
}

/// Read one request, write one response. Anything that goes wrong comes back as an
/// Error response rather than a dropped connection, so the Client can explain it.
async fn handle(mut stream: TcpStream, agent: Arc<Agent>) -> anyhow::Result<()> {
    let mut line = String::new();
    let mut reader = BufReader::new((&mut stream).take(PAIRING_LIMIT));
    tokio::time::timeout(PAIRING_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("Pairing request timed out")??;

    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => answer(request, &agent),
        Err(e) => Response::Error {
            message: format!("Could not understand that request: {e}"),
        },
    };

    let mut reply = serde_json::to_string(&response)?;
    reply.push('\n');
    stream.write_all(reply.as_bytes()).await?;

    Ok(())
}

fn answer(request: Request, agent: &Agent) -> Response {
    match request {
        Request::Info | Request::Health => Response::Error {
            message: "This Agent answers info and health only over SSH. Update Borrow on the Client and run borrow link again".to_string(),
        },
        Request::Pair {
            token,
            client,
            public_key,
            iroh,
            ..
        } => pair(
            agent,
            &token,
            &Client {
                name: client,
                public_key,
                iroh,
            },
        ),
    }
}

/// Client identity received during pairing. The Client also sends its own account name
/// and host keys, which nothing needs now that trust runs one way.
struct Client {
    name: String,
    public_key: String,
    iroh: Option<String>,
}

fn pair(agent: &Agent, token: &str, client: &Client) -> Response {
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
            message: "That pairing code has expired or was already used; run borrow serve again for a fresh one".to_string(),
        },
        Some(false) => Response::Error { message: "That pairing code is not right".to_string() },
        Some(true) => match accept(agent, client) {
            Ok(paired) => Response::Paired(Box::new(paired)),
            Err(e) => Response::Error { message: format!("Could not finish pairing: {e:#}") },
        },
    }
}

/// Authorize the Client key for SSH execution and private control, and its iroh key for
/// reaching sshd from other networks. An older Client without an iroh key pairs as before.
fn accept(agent: &Agent, client: &Client) -> anyhow::Result<Paired> {
    if let Some(iroh) = &client.iroh {
        clients::allow(&agent.root, &client.name, iroh)
            .context("Could not record the Client's iroh key")?;
    }
    let authorized = keys::authorize(&client.name, &client.public_key)
        .context("Could not add the Client's key to authorized_keys")?;

    info!("paired with {}", client.name);
    borrow_core::presentation::success(format!("Paired with {}", client.name));
    borrow_core::presentation::detail("Authorized", authorized.display());

    Ok(Paired {
        name: agent.name.clone(),
        user: agent.user.clone(),
        host_keys: keys::host_keys(),
        program: std::env::current_exe()
            .ok()
            .and_then(|path| path.to_str().map(str::to_string)),
        addresses: agent.addresses.clone(),
        iroh: Some(agent.iroh.clone()),
        specs: telemetry::specs(&agent.name),
    })
}

/// Generate a token using operating system randomness.
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

/// A label for an address, so somebody reading two pairing codes can tell which one
/// reaches them.
fn network_label(ip: IpAddr) -> &'static str {
    match Network::of(&ip.to_string()) {
        Network::ThisMachine => "This machine",
        Network::Local => "Local network",
        Network::Other => "Network",
        Network::Tailnet => "Tailscale",
    }
}

/// Print the pairing token only to the owner's console, never to logs or files.
/// Show a complete copyable command for each available address.
fn announce(name: &str, addresses: &[SocketAddr], token: &str) {
    let reachable: Vec<&SocketAddr> = addresses.iter().filter(|a| !a.ip().is_loopback()).collect();

    let offered = match reachable.is_empty() {
        true => addresses.iter().collect(),
        false => reachable,
    };

    eprintln!();
    borrow_core::presentation::progress(format!("Borrow is serving {name}"));
    eprintln!();

    if offered.is_empty() {
        eprintln!("  No address to pair on");
        return;
    }

    match offered.len() {
        1 => eprintln!("  On the other machine, run:"),
        _ => eprintln!("  On the other machine, run whichever reaches this box:"),
    }

    eprintln!();

    let commands: Vec<(String, &str)> = offered
        .iter()
        .map(|addr| {
            (
                format!("borrow link {}:{}:{token}", addr.ip(), addr.port()),
                network_label(addr.ip()),
            )
        })
        .collect();

    let width = commands.iter().map(|(c, _)| c.len()).max().unwrap_or(0);

    for (command, label) in &commands {
        eprintln!("      {command:<width$}   ({label})");
    }

    eprintln!();
    eprintln!(
        "  The code works once and expires in {} minutes",
        CODE_LIFETIME.as_secs() / 60
    );
    eprintln!("  Press Ctrl C to stop");
    eprintln!();
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    /// A private temporary Agent root removed when the test ends.
    pub struct Root(pub PathBuf);

    impl Root {
        pub fn new() -> Root {
            let path = std::env::temp_dir()
                .join(format!("borrow-agent-{}", borrow_core::storage::new_id()));
            borrow_core::storage::private_dir(&path).unwrap();
            Root(path.canonicalize().unwrap())
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
