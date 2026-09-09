//! `borrow serve`: the daemon that runs on the Agent.
//!
//! It answers questions about the box and hands out one key at pairing time. It
//! never runs your work: commands travel over ssh instead.

use borrow_core::keys;
use borrow_core::preflight;
use borrow_core::protocol::{Paired, Request, Response};
use borrow_core::telemetry;
use anyhow::Context;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
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

/// The private key this box uses to reach back to a Client for the mount.
const MOUNT_KEY_NAME: &str = "borrow_mount_ed25519";

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
                    if let Err(e) = handle(stream, agent, peer.ip()).await {
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
async fn handle(mut stream: TcpStream, agent: Arc<Agent>, peer: IpAddr) -> anyhow::Result<()> {
    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line).await?;

    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => answer(request, &agent, peer),
        Err(e) => Response::Error { message: format!("could not understand that request: {e}") },
    };

    let mut reply = serde_json::to_string(&response)?;
    reply.push('\n');
    stream.write_all(reply.as_bytes()).await?;

    Ok(())
}

fn answer(request: Request, agent: &Agent, peer: IpAddr) -> Response {
    match request {
        Request::Info => Response::Info(telemetry::specs(&agent.name)),
        Request::Health => Response::Health(telemetry::health()),
        Request::Pair { token, client, public_key, user, host_keys } => {
            pair(agent, &token, &Client { name: client, public_key, user, host_keys }, peer)
        }
    }
}

/// Trade a valid token for an installed key. The token is taken out of the daemon
/// before the key is written, so two Clients racing cannot both pair.
/// What the Client told us about itself at pairing.
struct Client {
    name: String,
    public_key: String,
    user: String,
    host_keys: Vec<String>,
}

fn pair(agent: &Agent, token: &str, client: &Client, peer: IpAddr) -> Response {
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
        Some(true) => match accept(agent, client, peer) {
            Ok(paired) => Response::Paired(paired),
            Err(e) => Response::Error { message: format!("could not finish pairing: {e:#}") },
        },
    }
}

/// Set up both directions of trust and describe the result.
///
/// Two independent one way trusts are established here and neither private key
/// moves. The Client gets to run commands on this box, and this box gets to reach
/// back for the mount. The second half is the new part in Phase 2, and it is why
/// the Client sends its own identity in the pairing request.
fn accept(agent: &Agent, client: &Client, peer: IpAddr) -> anyhow::Result<Paired> {
    let authorized = keys::authorize(&client.name, &client.public_key)
        .context("could not add the Client's key to authorized_keys")?;

    let known_hosts = keys::learn_host(&peer.to_string(), None, &client.host_keys)
        .context("could not record the Client's host keys")?;

    let (identity_file, mount_key) =
        mount_key().context("could not prepare this box's key for the mount")?;

    info!("paired with {}", client.name);
    eprintln!("✓ paired with {}", client.name);
    eprintln!("  authorized {}", authorized.display());
    eprintln!("  mount key  {}", identity_file.display());
    eprintln!("  will mount from {}@{}", client.user, peer);

    Ok(Paired {
        name: agent.name.clone(),
        user: agent.user.clone(),
        host_keys: keys::host_keys(),
        mount_key,
        mount_identity_file: identity_file.display().to_string(),
        mount_known_hosts: known_hosts.display().to_string(),
        client_address: peer.to_string(),
        specs: telemetry::specs(&agent.name),
    })
}

/// The key this box uses to pull the mount, made once and kept.
///
/// It is a separate key from anything the user owns, with no passphrase, because
/// the mount has to come up without a human present to unlock an agent. Keeping it
/// distinct is also what makes it revocable: it appears in the Client's
/// authorized_keys under borrow's own marker and nowhere else.
fn mount_key() -> anyhow::Result<(std::path::PathBuf, String)> {
    let private = keys::ssh_dir()?.join(MOUNT_KEY_NAME);
    let public = private.with_extension("pub");

    if !public.exists() {
        fs::create_dir_all(keys::ssh_dir()?)?;
        keys::set_mode(&keys::ssh_dir()?, 0o700)?;

        eprintln!("generating a mount key at {}", private.display());

        let status = std::process::Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-q", "-C", &keys::marker("mount"), "-f"])
            .arg(&private)
            .status()
            .context("could not run ssh-keygen")?;

        if !status.success() {
            anyhow::bail!("ssh-keygen failed while creating {}", private.display());
        }
    }

    let text = fs::read_to_string(&public)?;

    Ok((private, text.trim().to_string()))
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

/// A label for an address, so somebody reading two pairing codes can tell which
/// one reaches them. Derived from the address itself rather than from how it was
/// found, because a box routing everything over a VPN would otherwise mislabel it.
fn network_label(ip: IpAddr) -> &'static str {
    let IpAddr::V4(v4) = ip else {
        return "network";
    };

    let [first, second, ..] = v4.octets();

    match () {
        _ if v4.is_loopback() => "this machine",
        _ if first == 100 && (64..128).contains(&second) => "tailscale",
        _ if v4.is_private() => "local network",
        _ => "network",
    }
}

/// Print the pairing code once, to the owner's own console. It is never written to
/// a file and never logged, because whoever holds it can install a key.
///
/// Every address gets its own code rather than one code plus a footnote, because a
/// code you cannot paste is not a code.
fn announce(name: &str, addresses: &[SocketAddr], token: &str) {
    let reachable: Vec<&SocketAddr> =
        addresses.iter().filter(|a| !a.ip().is_loopback()).collect();

    let offered = match reachable.is_empty() {
        true => addresses.iter().collect(),
        false => reachable,
    };

    eprintln!();
    eprintln!("borrow is serving {name}");
    eprintln!();

    if offered.is_empty() {
        eprintln!("  no address to pair on");
        return;
    }

    match offered.len() {
        1 => eprintln!("  on the other machine, run:"),
        _ => eprintln!("  on the other machine, run whichever reaches this box:"),
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
    eprintln!("  the code works once and expires in {} minutes", CODE_LIFETIME.as_secs() / 60);
    eprintln!("  ctrl-c to stop");
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tailnet_address_is_named() {
        assert_eq!(network_label("100.67.90.119".parse().unwrap()), "tailscale");
    }

    #[test]
    fn a_home_network_address_is_named() {
        assert_eq!(network_label("10.0.0.193".parse().unwrap()), "local network");
        assert_eq!(network_label("192.168.1.7".parse().unwrap()), "local network");
    }

    /// 100.x is only a tailnet address inside the carrier grade NAT range. A public
    /// address that merely starts with 100 must not be mistaken for one.
    #[test]
    fn a_public_hundred_address_is_not_tailscale() {
        assert_eq!(network_label("100.20.0.1".parse().unwrap()), "network");
    }

    #[test]
    fn loopback_is_named_as_this_machine() {
        assert_eq!(network_label("127.0.0.1".parse().unwrap()), "this machine");
    }
}
