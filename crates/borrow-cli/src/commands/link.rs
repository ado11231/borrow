//! `borrow link <code>`: pair with a box and remember it.

use crate::client;
use crate::keys;
use crate::project;
use borrow_core::config::{Agent, Config};
use borrow_core::keys as core_keys;
use borrow_core::preflight::{self, Check};
use borrow_core::protocol::{Request, Response};
use borrow_core::tunnel;

/// Take a pairing code, install this machine's key on the box, and save what it
/// takes to reach it again. Every step says what it did.
///
/// One `ssh_port` feeds both the learned host keys and the saved Agent, because `unlink`
/// forgets those keys by the saved port. Learning them under a different one would leave
/// an entry nothing could ever remove.
pub async fn link(code: String, name: Option<String>) -> anyhow::Result<i32> {
    let (host, port, token) = parse_code(&code)?;
    let client_name = core_keys::client_name();

    let checks = vec![
        match preflight::is_listening(format!("{host}:{port}").parse()?) {
            true => Check::pass(format!("{host} is reachable")),
            false => Check::fail(
                format!("No response at {host}:{port}"),
                "Run borrow serve on the other machine".to_string(),
            ),
        },
        preflight::tool_check("rsync", Some("copying projects")),
    ];

    if preflight::report(&checks) {
        anyhow::bail!("Pairing stopped");
    }

    let (private_key, public_key) = keys::ensure(&client_name)?;
    let identity = tunnel::identity(&project::client_root()?)?;

    let response = client::pair(
        &host,
        port,
        Request::Pair {
            token,
            client: client_name.clone(),
            public_key,
            user: this_user(),
            host_keys: core_keys::host_keys(),
            iroh: Some(identity.public().to_string()),
        },
    )
    .await?;

    let Response::Paired(paired) = response else {
        anyhow::bail!("The Agent returned an unexpected response while pairing");
    };

    let name = name.unwrap_or(paired.name.clone());
    let ssh_port = None;
    let known_hosts = core_keys::learn_host(&host, ssh_port, &paired.host_keys)?;

    let mut config = Config::load_or_empty()?;
    config.upsert(Agent {
        name: name.clone(),
        host: host.clone(),
        user: paired.user.clone(),
        port: ssh_port,
        daemon_port: Some(port),
        identity_file: Some(private_key.clone()),
        known_hosts: Some(known_hosts.clone()),
        program: paired.program.clone(),
        addresses: paired.addresses.clone(),
        iroh: paired.iroh.clone(),
        specs: Some(paired.specs),
    });

    let saved = config.save()?;

    eprintln!();
    borrow_core::presentation::success(format!("Paired with {name}"));
    borrow_core::presentation::detail("Key", private_key.display());
    borrow_core::presentation::detail(
        "Installed",
        format!("{}@{}:~/.ssh/authorized_keys", paired.user, host),
    );
    borrow_core::presentation::detail(
        "Host keys",
        format!(
            "{} ({} learned)",
            known_hosts.display(),
            paired.host_keys.len()
        ),
    );
    if !paired.addresses.is_empty() {
        borrow_core::presentation::detail("Addresses", paired.addresses.join(", "));
    }
    borrow_core::presentation::detail("Saved", saved.display());
    eprintln!();
    eprintln!("  Try it:   borrow run uname -a");
    eprintln!("  In a project, borrow run copies its source to {name} first");
    eprintln!();

    Ok(0)
}

/// Split a pairing code into the box's address and the one time token. Codes are
/// readable on purpose, so you can see which machine you are about to trust.
fn parse_code(code: &str) -> anyhow::Result<(String, u16, String)> {
    let parts: Vec<&str> = code.trim().split(':').collect();

    let [host, port, token] = parts.as_slice() else {
        anyhow::bail!("That does not look like a pairing code. Expected host:port:code");
    };

    let port: u16 = port
        .parse()
        .map_err(|_| anyhow::anyhow!("'{port}' is not a port number"))?;

    Ok((host.to_string(), port, token.to_string()))
}

/// This account's name, sent for compatibility with older Agents.
fn this_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_code_splits_into_three_parts() {
        let (host, port, token) = parse_code("192.168.1.9:7433:K7QW9ZR2").unwrap();

        assert_eq!(host, "192.168.1.9");
        assert_eq!(port, 7433);
        assert_eq!(token, "K7QW9ZR2");
    }

    #[test]
    fn surrounding_whitespace_is_forgiven() {
        assert_eq!(parse_code("  10.0.0.4:7433:ABCD2345\n").unwrap().1, 7433);
    }

    #[test]
    fn a_code_missing_a_part_is_rejected() {
        assert!(parse_code("10.0.0.4:7433").is_err());
    }

    #[test]
    fn a_port_that_is_not_a_number_is_reported() {
        let err = parse_code("10.0.0.4:door:ABCD2345")
            .unwrap_err()
            .to_string();

        assert!(err.contains("door"), "message was: {err}");
    }
}
