//! `borrow sync [--pull] [--check] [path]`: move eligible source between machines.

use crate::project;
use crate::transfer::{self, Direction};
use borrow_core::config::Config;
use borrow_core::presentation;

pub async fn sync(
    agent: Option<String>,
    path: Option<std::path::PathBuf>,
    pull: bool,
    check: bool,
) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let local = project::require(path)?;
    let direction = match pull {
        true => Direction::Pull,
        false => Direction::Push,
    };

    let mut opened = transfer::open(target, &local).await?;
    let result = match (check, direction) {
        (true, _) if !opened.project.initialized => {
            presentation::progress(format!(
                "{} has no copy on {} yet. borrow sync will copy it",
                local.name, target.name
            ));
            Ok(())
        }
        (true, direction) => transfer::preview(&mut opened, target, &local, direction)
            .await
            .map(|_| ()),
        (false, Direction::Push) => transfer::push(&mut opened, target, &local)
            .await
            .map(|outcome| report(outcome, &target.name, &local.name, Direction::Push)),
        (false, Direction::Pull) => transfer::pull(&mut opened, target, &local)
            .await
            .map(|outcome| report(outcome, &target.name, &local.name, Direction::Pull)),
    };
    opened.control.close().await;
    result.map(|_| 0)
}

fn report(outcome: transfer::Outcome, agent: &str, name: &str, direction: Direction) {
    match (direction, outcome.changed) {
        (Direction::Push, 0) => {
            presentation::success(format!("{agent} already has the latest source of {name}"))
        }
        (Direction::Pull, 0) => presentation::success(format!(
            "This machine already has the latest source of {name}"
        )),
        (Direction::Push, _) => {}
        (Direction::Pull, changed) => presentation::success(format!(
            "Retrieved {} from {agent}",
            transfer::count(changed, "change")
        )),
    }
    if outcome.kept > 0 {
        let (place, hint) = match direction {
            Direction::Push => (agent.to_string(), "borrow sync --pull --check"),
            Direction::Pull => ("this machine".to_string(), "borrow sync --check"),
        };
        presentation::progress(format!(
            "Kept {} changed only on {place}. Review with {hint}",
            transfer::count(outcome.kept, "path")
        ));
    }
}
