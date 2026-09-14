//! `borrow unlink`: clean up this Client's Borrow data on a box, then forget it.

use crate::client::{Control, Refused};
use crate::project;
use crate::ssh::RemoteCommand;
use borrow_core::config::Config;
use borrow_core::control::{Request, Response};
use borrow_core::keys::marker;
use borrow_core::presentation;

/// Remote cleanup comes first and stops the unlink when the Agent refuses, for example
/// while this Client's projects still have a run, session, or sync in progress.
/// Cleanup is scoped to projects this Client registered, so other Clients keep theirs.
pub async fn unlink(agent: Option<String>) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();
    let tag = marker(&this_machine());
    let client_root = project::client_root()?;
    let registered = project::for_agent(&client_root, &target.name)?;
    let ids: Vec<String> = registered.iter().map(|p| p.id.clone()).collect();

    presentation::progress(format!("Removing Borrow access to {}", target.name));

    let cleanup = async {
        let mut control = Control::connect(&target).await?;
        let answer = control.call(Request::Unlink { projects: ids }).await;
        control.close().await;
        answer
    };
    let mut complete = true;
    match cleanup.await {
        Ok(Response::Unlinked { environment_files }) => presentation::success(format!(
            "Removed {} on {}. Source copies and backups were kept",
            crate::transfer::count(environment_files, "environment file"),
            target.name
        )),
        Ok(_) => return Err(crate::client::unexpected()),
        Err(error) if error.downcast_ref::<Refused>().is_some() => {
            anyhow::bail!("{error:#}. Nothing was unlinked")
        }
        Err(error) => {
            complete = false;
            presentation::warning(format!(
                "{error:#}. Remote cleanup is incomplete: environment files for this machine's projects remain in Borrow storage on {}",
                target.name
            ));
        }
    }

    if target.legacy_mount() {
        let unmount = RemoteCommand::to(
            &target,
            "sh".to_string(),
            vec!["-c".to_string(), unmount_script()],
        );
        match unmount.interactive().await {
            Ok(0) => {
                presentation::success(format!("Released older project mounts on {}", target.name))
            }
            _ => {
                complete = false;
                presentation::warning(format!(
                    "Could not release older project mounts on {}",
                    target.name
                ))
            }
        }
    }

    let script = removal_script(&tag);
    let mut remote = RemoteCommand::to(&target, "sh".to_string(), vec!["-c".to_string(), script]);
    remote.tty = false;
    match remote.interactive().await {
        Ok(0) => presentation::success(format!("Key removed from {}", target.name)),
        Ok(_) | Err(_) => {
            complete = false;
            presentation::warning(format!(
                "Could not reach {}, so the key is still there. Remove the line ending {tag} from its ~/.ssh/authorized_keys by hand",
                target.name
            ))
        }
    }

    borrow_core::keys::forget_host(&target.host, target.port)?;
    if target.legacy_mount() {
        borrow_core::keys::deauthorize(&target.name)?;
        presentation::success(format!(
            "Removed {}'s older mount key from this machine's authorized_keys",
            target.name
        ));
    }

    config.remove(&target.name)?;
    let saved = config.save()?;
    if complete {
        project::forget_agent(&client_root, &target.name)?;
    }

    presentation::success(format!("Forgot {}, saved {}", target.name, saved.display()));
    if !complete {
        presentation::warning(
            "Remote cleanup did not finish. Pair again and unlink once the Agent is reachable to finish it",
        );
    }

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

/// Release Phase 2 SSHFS mounts. `-z` detaches a mount even when something still has a
/// file open in it, which is the only thing that reliably clears a stale one.
fn unmount_script() -> String {
    let base = shell_words::quote(borrow_core::artifacts::LEGACY_MOUNT_BASE);

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
