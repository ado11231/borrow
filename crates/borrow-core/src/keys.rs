//! How borrow labels the key it installs, and how a machine publishes its identity.

use crate::config;
use anyhow::Context;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// The comment written into borrow's public key, and the marker `unlink` looks for
/// when taking that key back off a box. Both sides have to agree on it, which is
/// why it lives here rather than on either side.
pub fn marker(client_name: &str) -> String {
    format!("borrow:{client_name}")
}

/// Where sshd publishes the public half of a machine's host keys.
const HOST_KEY_DIR: &str = "/etc/ssh";

/// This machine's ssh host keys.
///
/// Both sides need this now. Sending them at pairing is what lets the very first
/// connection in either direction succeed without a human being asked to compare a
/// fingerprint they have no way of checking anyway.
pub fn host_keys() -> Vec<String> {
    let Ok(entries) = fs::read_dir(HOST_KEY_DIR) else {
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

/// How a host is written in a known_hosts file. Anything on a port other than the
/// usual one is wrapped in brackets, which is the format ssh expects.
fn host_pattern(host: &str, port: Option<u16>) -> String {
    match port {
        Some(port) if port != 22 => format!("[{host}]:{port}"),
        _ => host.to_string(),
    }
}

/// Record a box's ssh host keys so the first connection needs no fingerprint check.
/// Older entries are replaced, so pairing again after a rebuild still works.
pub fn learn_host(host: &str, port: Option<u16>, host_keys: &[String]) -> anyhow::Result<PathBuf> {
    let file = config::known_hosts_path()?;
    let pattern = host_pattern(host, port);

    let mut lines = without_host(&file, &pattern)?;

    for key in host_keys {
        let key = key.trim();

        match key.split_whitespace().collect::<Vec<&str>>().as_slice() {
            [kind, material, ..] => lines.push(format!("{pattern} {kind} {material}")),
            _ => continue,
        }
    }

    write_lines(&file, &lines)?;

    Ok(file)
}

/// Forget a box's host keys, so nothing is left pointing at a machine you unlinked.
pub fn forget_host(host: &str, port: Option<u16>) -> anyhow::Result<()> {
    let file = config::known_hosts_path()?;

    if !file.exists() {
        return Ok(());
    }

    let lines = without_host(&file, &host_pattern(host, port))?;
    write_lines(&file, &lines)
}

/// Every line of the file except the ones for this box.
fn without_host(file: &PathBuf, pattern: &str) -> anyhow::Result<Vec<String>> {
    let existing = fs::read_to_string(file).unwrap_or_default();

    Ok(existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| line.split_whitespace().next() != Some(pattern))
        .map(|line| line.to_string())
        .collect())
}

fn write_lines(file: &PathBuf, lines: &[String]) -> anyhow::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let mut body = lines.join("\n");
    body.push('\n');

    fs::write(file, body).with_context(|| format!("could not write {}", file.display()))
}

/// Add a public key to this machine's authorized_keys, replacing any older key
/// carrying the same marker.
///
/// Both machines call this now, once in each direction: the Agent authorises the
/// Client so `run` works, and the Client authorises the Agent so the mount can be
/// pulled. The file is rewritten whole, because appending to one with no trailing
/// newline welds two keys into a single broken line and locks you out.
///
/// The comment is **replaced** rather than trusted. Whatever the other machine
/// called its key, the line written here ends with the marker this machine will
/// search for when revoking it. A key that cannot be found again cannot be taken
/// back out, so the two must never be allowed to drift apart.
pub fn authorize(peer: &str, public_key: &str) -> anyhow::Result<PathBuf> {
    let tag = marker(peer);
    let key = authorized_line(peer, public_key)?;

    let dir = ssh_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    set_mode(&dir, 0o700)?;

    let file = dir.join("authorized_keys");
    let existing = fs::read_to_string(&file).unwrap_or_default();

    let mut lines: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.ends_with(&tag))
        .collect();

    lines.push(&key);

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
pub fn set_mode(path: &std::path::Path, mode: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("could not set permissions on {}", path.display()))?;
    }

    let _ = (path, mode);
    Ok(())
}

/// One authorized_keys line: the key itself, relabelled with our own marker.
fn authorized_line(peer: &str, public_key: &str) -> anyhow::Result<String> {
    match public_key.split_whitespace().collect::<Vec<&str>>().as_slice() {
        [kind, material, ..] if kind.starts_with("ssh-") || kind.starts_with("ecdsa-") => {
            Ok(format!("{kind} {material} {}", marker(peer)))
        }
        _ => anyhow::bail!("that does not look like an ssh public key"),
    }
}

/// Take a key back out of authorized_keys. What makes `unlink` mean something.
pub fn deauthorize(peer: &str) -> anyhow::Result<()> {
    let file = ssh_dir()?.join("authorized_keys");

    if !file.exists() {
        return Ok(());
    }

    let tag = marker(peer);
    let existing = fs::read_to_string(&file)?;

    let kept: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.ends_with(&tag))
        .collect();

    let mut body = kept.join("\n");
    body.push('\n');

    fs::write(&file, body).with_context(|| format!("could not write {}", file.display()))?;
    set_mode(&file, 0o600)
}

/// Where ssh keeps this account's keys.
pub fn ssh_dir() -> anyhow::Result<PathBuf> {
    let Some(base) = directories::BaseDirs::new() else {
        anyhow::bail!("could not determine home directory");
    };
    Ok(base.home_dir().join(".ssh"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usual_port_is_written_plainly() {
        assert_eq!(host_pattern("10.0.0.9", None), "10.0.0.9");
        assert_eq!(host_pattern("10.0.0.9", Some(22)), "10.0.0.9");
    }

    #[test]
    fn an_unusual_port_gets_brackets() {
        assert_eq!(host_pattern("10.0.0.9", Some(2222)), "[10.0.0.9]:2222");
    }

    #[test]
    fn the_marker_names_the_client() {
        assert_eq!(marker("laptop"), "borrow:laptop");
    }

    /// The bug this guards against: the Agent names its mount key `borrow:mount`,
    /// but the Client revokes it by the Agent's name. Written verbatim, the key
    /// installs fine and then cannot ever be removed.
    #[test]
    fn an_authorized_line_ends_with_the_marker_used_to_revoke_it() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 borrow:mount";

        let line = authorized_line("archbox", key).unwrap();

        assert!(line.ends_with(&marker("archbox")), "line was: {line}");
        assert!(line.starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5"), "line was: {line}");
    }

    #[test]
    fn a_key_with_no_comment_still_gets_one() {
        let line = authorized_line("archbox", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5").unwrap();

        assert_eq!(line, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 borrow:archbox");
    }

    #[test]
    fn something_that_is_not_a_key_is_refused() {
        assert!(authorized_line("archbox", "hello there").is_err());
    }
}
