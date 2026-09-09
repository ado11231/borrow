//! `borrow link <code>`: pair with a box and remember it.

use crate::client;
use borrow_core::config::{Agent, Config};
use crate::keys;
use borrow_core::keys as core_keys;
use borrow_core::preflight::{self, Check};
use borrow_core::protocol::{Request, Response};

/// Take a pairing code, install this machine's key on the box, and save what it
/// takes to reach it again. Every step says what it did.
pub async fn link(code: String, name: Option<String>) -> anyhow::Result<i32> {
    let (host, port, token) = parse_code(&code)?;
    let client_name = this_machine();

    let checks = vec![
        match preflight::is_listening(format!("{host}:{port}").parse()?) {
            true => Check::pass(format!("{host} is reachable")),
            false => Check::fail(
                format!("nothing answering at {host}:{port}"),
                "run borrow serve on the other machine".to_string(),
            ),
        },
        preflight::ssh_server_check(),
    ];

    if preflight::report(&checks) {
        anyhow::bail!("pairing stopped");
    }

    let (private_key, public_key) = keys::ensure(&client_name)?;

    let response = client::request(
        &host,
        port,
        Request::Pair {
            token,
            client: client_name.clone(),
            public_key,
            user: this_user(),
            host_keys: core_keys::host_keys(),
        },
    )
    .await?;

    let Response::Paired(paired) = response else {
        anyhow::bail!("the box answered something unexpected while pairing");
    };

    let name = name.unwrap_or(paired.name.clone());
    let known_hosts = core_keys::learn_host(&host, None, &paired.host_keys)?;
    let authorized = core_keys::authorize(&name, &paired.mount_key)?;

    let mut config = Config::load_or_empty()?;
    config.upsert(Agent {
        name: name.clone(),
        host: host.clone(),
        user: paired.user.clone(),
        port: None,
        daemon_port: Some(port),
        identity_file: Some(private_key.clone()),
        known_hosts: Some(known_hosts.clone()),
        mount_user: Some(this_user()),
        mount_host: Some(paired.client_address.clone()),
        mount_identity_file: Some(paired.mount_identity_file.clone()),
        mount_known_hosts: Some(paired.mount_known_hosts.clone()),
        specs: Some(paired.specs),
    });

    let saved = config.save()?;

    eprintln!();
    eprintln!("✓ paired with {name}");
    eprintln!("  key       {}", private_key.display());
    eprintln!("  installed {}@{}:~/.ssh/authorized_keys", paired.user, host);
    eprintln!("  host keys {} ({} learned)", known_hosts.display(), paired.host_keys.len());
    eprintln!("  saved     {}", saved.display());
    eprintln!();
    eprintln!("  and back the other way, so {name} can mount your files:");
    eprintln!("  authorized {}", authorized.display());
    eprintln!("  mounts from {}@{}", this_user(), paired.client_address);
    eprintln!();
    eprintln!("  try it:   borrow run uname -a");
    eprintln!();

    Ok(0)
}

/// Split a pairing code into the box's address and the one time token. Codes are
/// readable on purpose, so you can see which machine you are about to trust.
fn parse_code(code: &str) -> anyhow::Result<(String, u16, String)> {
    let parts: Vec<&str> = code.trim().split(':').collect();

    let [host, port, token] = parts.as_slice() else {
        anyhow::bail!("that does not look like a pairing code. expected host:port:code");
    };

    let port: u16 = port
        .parse()
        .map_err(|_| anyhow::anyhow!("'{port}' is not a port number"))?;

    Ok((host.to_string(), port, token.to_string()))
}

/// The account on this machine the Agent will log in as to pull the mount.
fn this_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// A name for this machine, used to label the key installed on the box so a human
/// reading authorized_keys can tell where it came from.
fn this_machine() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "client".to_string())
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
        let err = parse_code("10.0.0.4:door:ABCD2345").unwrap_err().to_string();

        assert!(err.contains("door"), "message was: {err}");
    }
}
