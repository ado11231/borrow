//! `borrow run <cmd>`: run a command on the Agent, in your project, split.

use crate::ssh::RemoteCommand;
use borrow_core::config::{Agent, Config};
use borrow_core::mount::{self, Layout};
use borrow_core::stack;

/// Run `cmd` on the Agent and return its exit code. A non zero code is not an
/// error: borrow did its job, and the command it ran happened to fail.
pub async fn run(agent: Option<String>, cmd: Vec<String>) -> anyhow::Result<i32> {
    let Some((program, args)) = cmd.split_first() else {
        anyhow::bail!("No command given. Try borrow run echo hello");
    };

    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;

    let mut remote = RemoteCommand::to(target, program.clone(), args.to_vec());

    let here = std::env::current_dir()?;
    let placement = place(target, &here)?;

    if let Some(placement) = &placement {
        remote.setup = placement.setup.clone();
        remote.cwd = Some(placement.layout.source.display().to_string());
        remote.env = placement.env.clone();
    }

    eprintln!(
        "{}",
        borrow_core::presentation::Style::stderr()
            .heading(announcement(&target.name, placement.as_ref()))
    );

    remote.execute().await
}

/// Everything that follows from being inside a project: where it appears on the
/// Agent, how it gets there, and what keeps its build output off the mount.
struct Placement {
    layout: Layout,
    setup: Vec<String>,
    env: Vec<(String, String)>,
    summary: Option<String>,
}

/// Work out where this command should run, or `None` when you are not inside a
/// project at all. Running from nowhere in particular is a normal thing to do:
/// `borrow run uname -a` should not need a `Cargo.toml` above it.
fn place(target: &Agent, here: &std::path::Path) -> anyhow::Result<Option<Placement>> {
    let Some(project) = stack::find(here) else {
        return Ok(None);
    };

    let Some((source, keys)) = target.mount_source(&project.root) else {
        anyhow::bail!(
            "{} was paired before borrow could mount your files. Run borrow link again to set up the return direction",
            target.name
        );
    };

    let layout = Layout::for_project(&project);
    let rules = mount::rules(&project, &layout);

    Ok(Some(Placement {
        setup: vec![
            mount::ensure_mounted(&layout, &source, &keys),
            mount::prepare(&layout, &rules),
        ],
        env: mount::env(&rules),
        summary: mount::summary(&rules),
        layout,
    }))
}

/// The line printed before anything runs. Saying where the work happens is a hard
/// requirement, and in Phase 2 that means saying where the files are too, so the
/// artifact split is something you can see rather than something you hope for.
fn announcement(name: &str, placement: Option<&Placement>) -> String {
    let mut line = format!("▶ Running on {name}");

    if let Some(placement) = placement {
        line.push_str(&format!(" · {}", placement.layout.source.display()));

        if let Some(summary) = &placement.summary {
            line.push_str(&format!(" · {summary}"));
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrow_core::stack::{Project, Stack};
    use std::path::PathBuf;

    fn placement(stacks: Vec<Stack>) -> Placement {
        let project = Project {
            root: PathBuf::from("/Users/me/app"),
            stacks,
        };
        let layout = Layout::for_project(&project);
        let rules = mount::rules(&project, &layout);

        Placement {
            setup: Vec::new(),
            env: mount::env(&rules),
            summary: mount::summary(&rules),
            layout,
        }
    }

    #[test]
    fn outside_a_project_it_only_says_where() {
        assert_eq!(announcement("archbox", None), "▶ Running on archbox");
    }

    #[test]
    fn inside_a_project_it_says_where_the_files_are_and_what_moved() {
        assert_eq!(
            announcement("archbox", Some(&placement(vec![Stack::Rust]))),
            "▶ Running on archbox · /mnt/borrow/app · target → local disk"
        );
    }

    #[test]
    fn a_project_with_no_known_stack_still_reports_its_path() {
        assert_eq!(
            announcement("archbox", Some(&placement(Vec::new()))),
            "▶ Running on archbox · /mnt/borrow/app"
        );
    }
}
