//! Building and running commands on the Agent over ssh.

use anyhow::Context;
use shell_words::join;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// The exit code reported when the remote command was killed by a signal rather than
/// exiting on its own. 130 is the shell convention for "terminated by SIGINT".
const EXIT_SIGNALLED: i32 = 130;

/// A command to run on the Agent. `cwd` and `env` are unused for now; they will carry
/// the mounted project path and the artifact split variables in Phase 2.
pub struct RemoteCommand {
    pub host: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

/// The arguments to pass to `ssh`: the host, then one command string. Each piece is
/// quoted first, so spaces stay inside their argument and characters and reach the remote shell as literal text.
impl RemoteCommand {
    pub fn to_ssh_args(&self) -> Vec<String> {
        let command = join(
            std::iter::once(self.program.as_str())
                .chain(self.args.iter().map(|s| s.as_str()))
        );

        vec![self.host.clone(), command]
    }

    /// Spawn `ssh`, stream both output pipes to this terminal as they produce lines, and
    /// return the remote command's exit code. Ctrl-C kills the ssh child, which drops the
    /// connection and hangs up the remote process.
    ///
    /// Both pipes are drained to EOF before the child is reaped, because a process can
    /// exit while bytes are still sitting in the OS buffer and reaping first loses them.
    pub async fn execute(&self) -> anyhow::Result<i32> {
        let mut child = Command::new("ssh")
            .args(self.to_ssh_args())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not spawn ssh; check that it is installed and on PATH")?;

        let stdout = child.stdout.take().context("ssh stdout was not captured")?;
        let stderr = child.stderr.take().context("ssh stderr was not captured")?;

        let mut out = BufReader::new(stdout).lines();
        let mut err = BufReader::new(stderr).lines();

        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);

        let mut out_open = true;
        let mut err_open = true;
        let mut interrupted = false;

        while out_open || err_open {
            tokio::select! {
                line = out.next_line(), if out_open => match line? {
                    Some(line) => println!("{line}"),
                    None => out_open = false,
                },
                line = err.next_line(), if err_open => match line? {
                    Some(line) => eprintln!("{line}"),
                    None => err_open = false,
                },
                _ = &mut interrupt, if !interrupted => {
                    interrupted = true;
                    child.start_kill().context("could not signal the ssh process")?;
                }
            }
        }

        let status = child.wait().await.context("waiting on ssh failed")?;

        Ok(status.code().unwrap_or(EXIT_SIGNALLED))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn cmd(args: &[&str]) -> String {
    RemoteCommand {
        host: "localbox".to_string(),
        program: "echo".to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: None,
        env: Vec::new(),
    }
    .to_ssh_args()[1]
    .clone()
}

    #[test]
    fn quotes_nothing_when_unnecessary() {
        assert_eq!(cmd(&["hello"]), "echo hello");
    }

    #[test]
    fn quotes_arguments_containing_spaces() {
        assert_eq!(cmd(&["hello world"]), "echo 'hello world'");
    }

    #[test]
    fn handles_embedded_single_quote() {
        assert_eq!(cmd(&["it's"]), r"echo 'it'\''s'");
    }

    #[test]
    fn dollar_sign_stays_literal() {
        assert_eq!(cmd(&["$HOME"]), "echo '$HOME'");
    }

    #[test]
    fn semicolon_stays_literal() {
        assert_eq!(cmd(&["a; whoami"]), "echo 'a; whoami'");
    }
}