//! `slingshot run <cmd>`: sync the project, then run a command in the Agent copy.

use crate::client::{self, Refused};
use crate::project::{self, Local};
use crate::route;
use crate::ssh::{Disconnected, RemoteCommand};
use crate::transfer;
use slingshot_core::artifacts;
use slingshot_core::config::{Agent, Config};
use slingshot_core::control::{Request, Response};
use slingshot_core::presentation::{self, Style};

/// Run `cmd` on the Agent and return its exit code. A non zero code is not an
/// error: slingshot did its job, and the command it ran happened to fail.
pub async fn run(agent: Option<String>, cmd: Vec<String>) -> anyhow::Result<i32> {
    if cmd.is_empty() {
        anyhow::bail!("No command given. Try slingshot run echo hello");
    }

    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let local = project::locate(&std::env::current_dir()?);

    let mut args = vec!["internal-run".to_string()];
    let warnings = match &local {
        Some(local) => {
            let mut opened = transfer::open(target, local).await?;
            transfer::push(&mut opened, target, local).await?;
            let warnings = opened.control.call(Request::Warnings).await;
            opened.control.close().await;
            args.extend(["--project".to_string(), opened.id]);
            if !local.cwd.is_empty() {
                args.extend(["--cwd".to_string(), local.cwd.clone()]);
            }
            warnings
        }
        None => client::request(target, Request::Warnings).await,
    };
    show_warnings(warnings);

    args.push("--".to_string());
    args.extend(cmd);

    eprintln!(
        "{}",
        Style::stderr().heading(announcement(
            &target.name,
            route::resolve(target).name(),
            local.as_ref()
        ))
    );

    let remote = RemoteCommand::to(target, target.program().to_string(), args);
    interact(target, &remote, lost_connection(&target.name)).await
}

/// Run an interactive remote command, replacing SSH's messages about a dropped connection
/// with `lost`. When SSH exits 255 silently, a quick check tells a keepalive timeout
/// apart from a command that really exited with 255.
pub async fn interact(target: &Agent, remote: &RemoteCommand, lost: String) -> anyhow::Result<i32> {
    let error = match remote.interactive().await {
        Ok(code) => return Ok(code),
        Err(error) => error,
    };
    match error.downcast_ref::<Disconnected>() {
        Some(Disconnected::Certain) => anyhow::bail!(lost),
        Some(Disconnected::Possible) if RemoteCommand::reachable(target).await => Ok(255),
        Some(_) => anyhow::bail!(lost),
        None => Err(error),
    }
}

/// Said instead of SSH's own messages. A run belongs to its connection, so the Agent
/// stops it once it notices, and sessions are the way to outlive a disconnect.
fn lost_connection(name: &str) -> String {
    format!(
        "Lost connection to {name}. The Agent stops the run once it notices, unless it finishes first. See how it ended with slingshot ps --all, and use slingshot attach for work that must survive a disconnect"
    )
}

/// Show resource warnings. A failed check is ignored, because it must never block work,
/// but an Agent that refuses the request is worth mentioning.
pub fn show_warnings(result: anyhow::Result<Response>) {
    match result {
        Ok(Response::Warnings(warnings)) => {
            for warning in warnings {
                presentation::warning(warning);
            }
        }
        Err(error) if error.downcast_ref::<Refused>().is_some() => {
            presentation::warning(format!("Could not check Agent resources: {error:#}"))
        }
        _ => {}
    }
}

/// The line printed before anything runs. Saying where the work happens is a hard
/// requirement, including the path taken, which project folder, and what build output moved.
fn announcement(name: &str, path: &str, local: Option<&Local>) -> String {
    let mut line = format!("▶ Running on {name} via {path}");

    let Some(local) = local else {
        return line;
    };
    line.push_str(&format!(" · {}", location(local)));
    if let Some(summary) = split_summary(local) {
        line.push_str(&format!(" · {summary}"));
    }
    line
}

pub fn location(local: &Local) -> String {
    match local.cwd.is_empty() {
        true => local.name.clone(),
        false => format!("{}/{}", local.name, local.cwd),
    }
}

fn split_summary(local: &Local) -> Option<String> {
    artifacts::summary(&local.stacks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slingshot_core::stack::Stack;
    use std::path::PathBuf;

    fn local(cwd: &str, stacks: Vec<Stack>) -> Local {
        Local {
            root: PathBuf::from("/work/app"),
            name: "app".to_string(),
            cwd: cwd.to_string(),
            stacks,
        }
    }

    #[test]
    fn outside_a_project_it_only_says_where() {
        assert_eq!(
            announcement("archbox", "local network", None),
            "▶ Running on archbox via local network"
        );
    }

    #[test]
    fn inside_a_project_it_names_the_folder_and_what_moved() {
        assert_eq!(
            announcement(
                "archbox",
                "tailnet",
                Some(&local("crates/cli", vec![Stack::Rust]))
            ),
            "▶ Running on archbox via tailnet · app/crates/cli · target → Agent disk"
        );
    }

    #[test]
    fn a_lost_connection_names_the_box_and_where_to_look() {
        let message = lost_connection("archbox");
        assert!(
            message.starts_with("Lost connection to archbox."),
            "{message}"
        );
        assert!(message.contains("slingshot ps --all"), "{message}");
    }

    #[test]
    fn a_project_with_no_known_stack_still_reports_its_folder() {
        assert_eq!(
            announcement("archbox", "local network", Some(&local("", Vec::new()))),
            "▶ Running on archbox via local network · app"
        );
    }
}
