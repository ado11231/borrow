//! `borrow unlink`: take the key back off a box and forget it.

use borrow_core::config::Config;
use borrow_core::keys::marker;
use crate::keys;
use crate::ssh::RemoteCommand;

/// Remove borrow's key from the box's authorized_keys, then drop it from the config.
/// The key is found by the marker written into its comment at pairing, so nothing
/// else in that file is touched.
pub async fn unlink(agent: Option<String>) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();
    let tag = marker(&this_machine());

    eprintln!("▶ removing borrow's key on {}", target.name);

    let script = removal_script(&tag);
    let remote = RemoteCommand::to(&target, "sh".to_string(), vec!["-c".to_string(), script]);

    match remote.execute().await {
        Ok(0) => eprintln!("✓ key removed from {}", target.name),
        Ok(_) | Err(_) => eprintln!(
            "! could not reach {}, so the key is still there. remove the line ending {tag} from its ~/.ssh/authorized_keys by hand",
            target.name
        ),
    }

    keys::forget_host(&target.host, target.port)?;
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

fn this_machine() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "client".to_string())
}
