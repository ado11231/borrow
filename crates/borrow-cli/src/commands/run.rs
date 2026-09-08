//! `borrow run <cmd>`: run a command on the Agent.

use borrow_core::config::Config;
use crate::ssh::RemoteCommand;

/// Run `cmd` on the Agent and return its exit code. A non zero code is not an
/// error: borrow did its job, and the command it ran happened to fail.
pub async fn run(agent: Option<String>, cmd: Vec<String>) -> anyhow::Result<i32> {
    let Some((program, args)) = cmd.split_first() else {
        anyhow::bail!("no command given. try borrow run echo hello");
    };

    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;

    let remote = RemoteCommand::to(target, program.clone(), args.to_vec());

    eprintln!("▶ running on {}", target.name);

    remote.execute().await
}
