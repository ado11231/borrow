//! Borrow uses a dedicated SSH key so unlink can revoke its access.

use anyhow::Context;
use borrow_core::keys::{marker, ssh_dir};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The private key file. The public one is the same path with .pub on the end.
const KEY_NAME: &str = "borrow_ed25519";

fn private_key_path() -> anyhow::Result<PathBuf> {
    Ok(ssh_dir()?.join(KEY_NAME))
}

/// Find borrow's key, creating it the first time. Generating one is announced with
/// its path, because a tool that quietly makes keys cannot be audited.
pub fn ensure(client_name: &str) -> anyhow::Result<(PathBuf, String)> {
    let private = private_key_path()?;
    let public = private.with_extension("pub");

    if !public.exists() {
        let dir = ssh_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("Could not create {}", dir.display()))?;

        eprintln!("Generating a Borrow SSH key at {}", private.display());

        let status = Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-N",
                "",
                "-q",
                "-C",
                &marker(client_name),
                "-f",
            ])
            .arg(&private)
            .status()
            .context("Could not run ssh-keygen; check that ssh is installed")?;

        if !status.success() {
            anyhow::bail!(
                "SSH key generation failed while creating {}",
                private.display()
            );
        }
    }

    let text = fs::read_to_string(&public)
        .with_context(|| format!("Could not read {}", public.display()))?;

    Ok((private, text.trim().to_string()))
}
