//! `borrow run <cmd>`: sync the project, then run a command in the Agent copy.

use crate::client::{self, Refused};
use crate::project::{self, Local};
use crate::ssh::RemoteCommand;
use crate::transfer;
use borrow_core::artifacts::{self, Layout};
use borrow_core::config::Config;
use borrow_core::control::{Request, Response};
use borrow_core::presentation::{self, Style};
use borrow_core::stack;

/// Run `cmd` on the Agent and return its exit code. A non zero code is not an
/// error: borrow did its job, and the command it ran happened to fail.
pub async fn run(agent: Option<String>, cmd: Vec<String>) -> anyhow::Result<i32> {
    if cmd.is_empty() {
        anyhow::bail!("No command given. Try borrow run echo hello");
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
        Style::stderr().heading(announcement(&target.name, local.as_ref()))
    );

    RemoteCommand::to(target, target.program().to_string(), args)
        .interactive()
        .await
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
/// requirement, including which project folder and what build output moved.
pub fn announcement(name: &str, local: Option<&Local>) -> String {
    let mut line = format!("▶ Running on {name}");

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
    let project = stack::Project {
        root: local.root.clone(),
        stacks: local.stacks.clone(),
    };
    let layout = Layout {
        source: local.root.clone(),
        artifacts: local.root.join(".borrow-artifacts"),
    };
    artifacts::summary(&artifacts::rules(&project, &layout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrow_core::stack::Stack;
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
        assert_eq!(announcement("archbox", None), "▶ Running on archbox");
    }

    #[test]
    fn inside_a_project_it_names_the_folder_and_what_moved() {
        assert_eq!(
            announcement("archbox", Some(&local("crates/cli", vec![Stack::Rust]))),
            "▶ Running on archbox · app/crates/cli · target → local disk"
        );
    }

    #[test]
    fn a_project_with_no_known_stack_still_reports_its_folder() {
        assert_eq!(
            announcement("archbox", Some(&local("", Vec::new()))),
            "▶ Running on archbox · app"
        );
    }
}
