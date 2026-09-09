//! The ssh key borrow uses, and nothing else.
//!
//! borrow keeps its own key rather than reusing your personal one, so the key on a
//! box is clearly borrow's and `unlink` can take it back out.

use anyhow::Context;
use borrow_core::keys::marker;
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

/// Find borrow's key, creating it the first time. Generating one is announced with
/// its path, because a tool that quietly makes keys cannot be audited.
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

