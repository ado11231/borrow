//! `borrow unlink`: take the key back off a box and forget it.

use crate::ssh::RemoteCommand;
use borrow_core::config::Config;
use borrow_core::keys::marker;

/// Unmount before revoking either key so live mounts can be released.
/// Find installed keys by their Borrow markers.
pub async fn unlink(agent: Option<String>) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();
    let tag = marker(&this_machine());

    borrow_core::presentation::progress(format!(
        "Unmounting and removing borrow's key on {}",
        target.name
    ));

    let unmount = RemoteCommand::to(
        &target,
        "sh".to_string(),
        vec!["-c".to_string(), unmount_script()],
    );

    match unmount.execute().await {
        Ok(0) => borrow_core::presentation::success(format!("Mounts released on {}", target.name)),
        Ok(_) | Err(_) => borrow_core::presentation::warning(format!(
            "Could not release mounts on {}",
            target.name
        )),
    }

    let script = removal_script(&tag);
    let remote = RemoteCommand::to(&target, "sh".to_string(), vec!["-c".to_string(), script]);

    match remote.execute().await {
        Ok(0) => borrow_core::presentation::success(format!("Key removed from {}", target.name)),
        Ok(_) | Err(_) => borrow_core::presentation::warning(format!(
            "Could not reach {}, so the key is still there. Remove the line ending {tag} from its ~/.ssh/authorized_keys by hand",
            target.name
        )),
    }

    borrow_core::keys::forget_host(&target.host, target.port)?;
    borrow_core::keys::deauthorize(&target.name)?;
    borrow_core::presentation::success(format!(
        "Removed {}'s key from this machine's authorized_keys",
        target.name
    ));

    config.remove(&target.name)?;
    let saved = config.save()?;

    borrow_core::presentation::success(format!(
        "Forgot {}, saved {}",
        target.name,
        saved.display()
    ));

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
