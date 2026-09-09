//! `borrow unlink`: take the key back off a box and forget it.

use borrow_core::config::Config;
use borrow_core::keys::marker;
use crate::ssh::RemoteCommand;

/// Undo pairing in both directions, then forget the box.
///
/// Order matters. The mount comes down first, because taking the Agent's key out of
/// this machine's authorized_keys while a mount is live leaves one that hangs and
/// can no longer reconnect. Each key is found by the marker written into its
/// comment at pairing, so nothing else in either file is touched.
pub async fn unlink(agent: Option<String>) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();
    let tag = marker(&this_machine());

    eprintln!("▶ unmounting and removing borrow's key on {}", target.name);

    let unmount = RemoteCommand::to(
        &target,
        "sh".to_string(),
        vec!["-c".to_string(), unmount_script()],
    );

    match unmount.execute().await {
        Ok(0) => eprintln!("✓ mounts released on {}", target.name),
        Ok(_) | Err(_) => eprintln!("! could not release mounts on {}", target.name),
    }

    let script = removal_script(&tag);
    let remote = RemoteCommand::to(&target, "sh".to_string(), vec!["-c".to_string(), script]);

    match remote.execute().await {
        Ok(0) => eprintln!("✓ key removed from {}", target.name),
        Ok(_) | Err(_) => eprintln!(
            "! could not reach {}, so the key is still there. remove the line ending {tag} from its ~/.ssh/authorized_keys by hand",
            target.name
        ),
    }

    borrow_core::keys::forget_host(&target.host, target.port)?;
    borrow_core::keys::deauthorize(&target.name)?;
    eprintln!("✓ removed {}'s key from this machine's authorized_keys", target.name);

    config.remove(&target.name)?;
    let saved = config.save()?;

    eprintln!("✓ forgot {}, saved {}", target.name, saved.display());

    Ok(0)
}

/// A small shell script that rewrites authorized_keys without our line. It writes
/// through the original file rather than replacing it, so the file keeps the
/// permissions ssh insists on.
fn removal_script(marker: &str) -> String {
    let pattern = shell_words::quote(marker);

    format!(
        "f=$HOME/.ssh/authorized_keys; \
         [ -f \"$f\" ] || exit 0; \
         t=$(mktemp) && grep -F -v -e {pattern} \"$f\" > \"$t\"; \
         cat \"$t\" > \"$f\" && rm -f \"$t\""
    )
}

/// Unmount everything borrow put under its mount base and take the directories
/// away. `-z` detaches a mount even when something still has a file open in it,
/// which is the only thing that reliably clears one that has gone stale.
fn unmount_script() -> String {
    let base = shell_words::quote(borrow_core::mount::MOUNT_BASE);

    format!(
        "[ -d {base} ] || exit 0; \
         for d in {base}/*; do \
           [ -d \"$d\" ] || continue; \
           fusermount -u -z \"$d\" >/dev/null 2>&1 || true; \
           rmdir \"$d\" >/dev/null 2>&1 || true; \
         done"
    )
}

fn this_machine() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "client".to_string())
}
