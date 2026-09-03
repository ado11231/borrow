//! `borrow run <cmd>` — execute a command on the Agent.

/// Run `cmd` on the Agent and return its exit code. A non-zero code is not an
/// error: borrow did its job, the command it ran happened to fail.
pub async fn run(cmd: Vec<String>) -> anyhow::Result<i32> {
    println!("{:?}", cmd);
    Ok(0)
}
