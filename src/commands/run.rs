//! `borrow run <cmd>` execute a command on the Agent.
use crate::ssh;

/// Run `cmd` on the Agent and return its exit code. A non-zero code is not an
/// error: borrow did its job, the command it ran happened to fail.
///
/// The host is hardcoded until `config.rs` and pairing land.
pub async fn run(cmd: Vec<String>) -> anyhow::Result<i32> {
    let Some((program, args)) = cmd.split_first() else {
        anyhow::bail!("No Command Given");
    };

    let remote = ssh::RemoteCommand {
        host: "archbox".to_string(),
        program: program.clone(),
        args: args.to_vec(),
        cwd: None,
        env: Vec::new(),
    };

    eprintln!("▶ running on {}", remote.host);

    remote.execute().await
}
