//! `slingshot attach [path]`: open or rejoin the project's persistent session.

use crate::project;
use crate::route;
use crate::ssh::{self, RemoteCommand};
use crate::transfer::{self, Direction};
use slingshot_core::config::Config;
use slingshot_core::control::{Request, Response};
use slingshot_core::presentation::{Style, Tone};

/// Copy source only on first use. Reattaching never syncs, so a running session keeps
/// exactly the files it started with until the user syncs deliberately.
pub async fn attach(
    agent: Option<String>,
    path: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    if !ssh::wants_terminal() {
        anyhow::bail!("slingshot attach needs an interactive terminal");
    }
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let local = project::require(path)?;

    let mut opened = transfer::open(target, &local).await?;
    if !opened.project.initialized {
        let step = transfer::syncing(&local);
        let outcome = transfer::push(&mut opened, target, &local, &step).await?;
        transfer::finish(step, &outcome, Direction::Push);
    }
    let Response::Session(session) = opened
        .control
        .call(Request::Session {
            project: opened.id.clone(),
        })
        .await?
    else {
        return Err(crate::client::unexpected());
    };
    if session.created {
        let warnings = opened.control.call(Request::Warnings).await;
        super::run::show_warnings(warnings);
    }
    opened.control.close().await;

    let verb = match session.created {
        true => "Starting",
        false => "Attaching to",
    };
    let style = Style::stderr();
    eprintln!(
        "{} {verb} a session on {} via {} · {}",
        style.paint("▶", Tone::Info),
        style.paint(&target.name, Tone::Info),
        style.paint(route::resolve(target).name(), Tone::Info),
        local.name
    );
    eprintln!(
        "  {}",
        style.dim("Detach with Ctrl B then D, or your tmux prefix then D")
    );

    let mut remote = RemoteCommand::to(
        target,
        session.tmux,
        vec![
            "-S".to_string(),
            session.socket,
            "attach-session".to_string(),
            "-t".to_string(),
            format!("={}", session.job.id),
        ],
    );
    remote.tty = true;
    let lost = format!(
        "Lost connection to {}. The session keeps running there. Run slingshot attach again to return to it",
        target.name
    );
    super::run::interact(target, &remote, lost).await
}
