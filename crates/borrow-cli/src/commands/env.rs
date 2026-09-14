//! `borrow env`: environment files kept on the Agent, outside source copies.

use crate::client::{Control, unexpected};
use crate::project;
use borrow_core::config::Config;
use borrow_core::control::{MAX_ENVIRONMENT_FILE, Request, Response};
use borrow_core::presentation::{self, Style};
use std::path::PathBuf;

/// Contents travel inside the control message over SSH input, never as command
/// arguments, and are never printed.
pub async fn add(
    agent: Option<String>,
    file: PathBuf,
    target: String,
    replace: bool,
) -> anyhow::Result<i32> {
    borrow_agent::projects::environment_target(&target)?;
    let meta = std::fs::metadata(&file)
        .map_err(|e| anyhow::anyhow!("Could not read {}: {e}", file.display()))?;
    anyhow::ensure!(meta.is_file(), "{} is not a file", file.display());
    anyhow::ensure!(
        meta.len() <= MAX_ENVIRONMENT_FILE as u64,
        "Environment files are limited to 1 MiB"
    );
    let contents = std::fs::read(&file)?;
    let (config, local) = (Config::load()?, project::require(None)?);
    let target_agent = config.resolve(agent.as_deref())?;
    let id = project::identify(&project::client_root()?, &local.root, &target_agent.name)?;
    let mut control = Control::connect(target_agent).await?;
    let answer = control
        .call(Request::EnvAdd {
            project: id,
            target: target.clone(),
            contents,
            replace,
        })
        .await;
    control.close().await;
    answer?;
    presentation::success(format!(
        "Set {target} for {} on {}",
        local.name, target_agent.name
    ));
    Ok(0)
}

pub async fn list(agent: Option<String>) -> anyhow::Result<i32> {
    let (config, local) = (Config::load()?, project::require(None)?);
    let target = config.resolve(agent.as_deref())?;
    let id = project::identify(&project::client_root()?, &local.root, &target.name)?;
    let Response::Names(names) =
        crate::client::request(target, Request::EnvList { project: id }).await?
    else {
        return Err(unexpected());
    };
    let style = Style::stdout();
    println!(
        "{}",
        style.heading(format!(
            "Environment files for {} on {}",
            local.name, target.name
        ))
    );
    if names.is_empty() {
        println!("  None set. Add one with borrow env add --file <file> --target .env");
    }
    for name in names {
        println!("  {name}");
    }
    Ok(0)
}

pub async fn remove(agent: Option<String>, target: String) -> anyhow::Result<i32> {
    borrow_agent::projects::environment_target(&target)?;
    let (config, local) = (Config::load()?, project::require(None)?);
    let target_agent = config.resolve(agent.as_deref())?;
    let id = project::identify(&project::client_root()?, &local.root, &target_agent.name)?;
    crate::client::request(
        target_agent,
        Request::EnvRemove {
            project: id,
            target: target.clone(),
        },
    )
    .await?;
    presentation::success(format!(
        "Removed {target} for {} on {}",
        local.name, target_agent.name
    ));
    Ok(0)
}
