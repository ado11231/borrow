//! The ssh key borrow uses, and nothing else.
//!
//! borrow keeps its own key rather than reusing your personal one. That way the key
//! installed on a box is clearly borrow's, a human can spot it in authorized_keys,
//! and `borrow unlink` can take it back out without touching anything you own.

use crate::config;
use anyhow::Context;
use directories::BaseDirs;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The private key file. The public one is the same path with .pub on the end.
const KEY_NAME: &str = "borrow_ed25519";

/// Where ssh keeps keys. The only place in the codebase that knows this.
fn ssh_dir() -> anyhow::Result<PathBuf> {
    let Some(base) = BaseDirs::new() else {
        anyhow::bail!("could not determine home directory");
    };
    Ok(base.home_dir().join(".ssh"))
}

/// Path to borrow's private key.
pub fn private_key_path() -> anyhow::Result<PathBuf> {
    Ok(ssh_dir()?.join(KEY_NAME))
}

/// The comment written into the public key, which is also the marker `unlink`
/// searches for when removing the key from a box later on.
pub fn marker(client_name: &str) -> String {
    format!("borrow:{client_name}")
}

/// Find borrow's key, creating it the first time. Generating a key is announced out
/// loud with the path, because a tool that quietly makes keys is a tool you cannot
/// audit. Returns the private key path and the public key text to send over.
pub fn ensure(client_name: &str) -> anyhow::Result<(PathBuf, String)> {
    let private = private_key_path()?;
    let public = private.with_extension("pub");

    if !public.exists() {
        let dir = ssh_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;

        eprintln!("generating a new ssh key for borrow at {}", private.display());

        let status = Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-q", "-C", &marker(client_name), "-f"])
            .arg(&private)
            .status()
            .context("could not run ssh-keygen; check that ssh is installed")?;

        if !status.success() {
            anyhow::bail!("ssh-keygen failed while creating {}", private.display());
        }
    }

    let text = fs::read_to_string(&public)
        .with_context(|| format!("could not read {}", public.display()))?;

    Ok((private, text.trim().to_string()))
}

/// How a host is written in a known_hosts file. Anything on a port other than the
/// usual one is wrapped in brackets, which is the format ssh expects.
fn host_pattern(host: &str, port: Option<u16>) -> String {
    match port {
        Some(port) if port != 22 => format!("[{host}]:{port}"),
        _ => host.to_string(),
    }
}

/// Record a box's ssh host keys so the first connection works without anybody being
/// asked to check a fingerprint. Older entries for the same box are replaced, which
/// is what makes pairing again after a rebuild just work.
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
}
