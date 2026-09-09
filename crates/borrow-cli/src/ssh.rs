//! Building and running commands on the Agent over ssh.

use borrow_core::config;
use anyhow::Context;
use shell_words::{join, quote};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// The exit code reported when the remote command was killed by a signal rather than
/// exiting on its own. 130 is the shell convention for "terminated by SIGINT".
const EXIT_SIGNALLED: i32 = 130;

/// Options passed to every ssh call. BatchMode fails loudly rather than quietly
/// asking for a password. The alive settings keep a long build from dying on an
/// idle connection. LogLevel hides ssh's own chatter, such as the closing notice
/// after every command, while leaving real failures visible.
const SSH_OPTIONS: &[&str] = &[
    "BatchMode=yes",
    "ConnectTimeout=10",
    "ServerAliveInterval=30",
    "ServerAliveCountMax=6",
    "LogLevel=ERROR",
];

/// Whether to ask ssh for a terminal on the box.
///
/// A terminal is what makes ctrl-c stop the remote command, but it merges stderr
/// into stdout and ends lines with \r\n. So ask for one only when a person is at
/// the keyboard, and keep clean separate streams whenever output is redirected.
fn wants_terminal() -> bool {
    use std::io::IsTerminal;

    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// A command to run on the Agent.
///
/// `setup` holds shell lines that must succeed before the command runs, such as
/// bringing the mount up. `cwd` is the mounted project on the Agent, and `env`
/// carries the artifact split variables that keep build output off it.
pub struct RemoteCommand {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
    /// Whether to ask for a terminal on the box. See `wants_terminal`.
    pub tty: bool,
    pub program: String,
    pub args: Vec<String>,
    /// Shell lines run first, joined so that a failure stops everything after it.
    pub setup: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

impl RemoteCommand {
    /// Build a command aimed at a saved box. Everything ssh needs comes from the
    /// config, so nobody has to keep an entry in `~/.ssh/config` in step with this.
    pub fn to(agent: &config::Agent, program: String, args: Vec<String>) -> RemoteCommand {
        RemoteCommand {
            host: agent.host.clone(),
            user: Some(agent.user.clone()),
            port: agent.port,
            identity_file: agent.identity_file.clone(),
            known_hosts: agent.known_hosts.clone(),
            tty: wants_terminal(),
            program,
            args,
            setup: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    /// Who to log in as and where, in the form ssh expects.
    fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    /// The single string the remote shell will parse.
    ///
    /// Every piece the user supplied is quoted first, so characters like `$` and
    /// `;` arrive as text instead of being run. The parts are joined with `&&`
    /// rather than `;` on purpose: if the mount fails to come up, the build must
    /// not then run in an empty directory and appear to succeed at nothing.
    pub fn command_line(&self) -> String {
        let user_command = join(
            std::iter::once(self.program.as_str())
                .chain(self.args.iter().map(|s| s.as_str())),
        );

        let mut parts = self.setup.clone();

        if let Some(cwd) = &self.cwd {
            parts.push(format!("cd {}", quote(cwd)));
        }

        parts.push(match self.env.is_empty() {
            true => user_command,
            false => {
                let assignments = self
                    .env
                    .iter()
                    .map(|(key, value)| quote(&format!("{key}={value}")).into_owned())
                    .collect::<Vec<String>>()
                    .join(" ");

                format!("env {assignments} {user_command}")
            }
        });

        parts.join(" && ")
    }

    /// The full argument list for `ssh`. Options first, then the destination, then
    /// the command, because ssh treats everything after the destination as payload.
    pub fn to_ssh_args(&self) -> Vec<String> {
        let mut argv = Vec::new();

        for option in SSH_OPTIONS {
            argv.push("-o".to_string());
            argv.push(option.to_string());
        }

        if let Some(port) = self.port {
            argv.push("-p".to_string());
            argv.push(port.to_string());
        }

        if let Some(key) = &self.identity_file {
            argv.push("-i".to_string());
            argv.push(key.display().to_string());
            argv.push("-o".to_string());
            argv.push("IdentitiesOnly=yes".to_string());
        }

        if let Some(known_hosts) = &self.known_hosts {
            argv.push("-o".to_string());
            argv.push(format!("UserKnownHostsFile=\"{}\"", known_hosts.display()));
            argv.push("-o".to_string());
            argv.push("StrictHostKeyChecking=yes".to_string());
        }

        if self.tty {
            argv.push("-t".to_string());
        }

        argv.push(self.destination());
        argv.push(self.command_line());

        argv
    }

    /// Spawn `ssh`, stream both pipes as lines arrive, and return the remote exit code.
    /// Ctrl-c kills the ssh child. Both pipes are drained to EOF before the child is
    /// reaped, because a process can exit with bytes still sitting in the OS buffer.
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
    
    fn remote(args: &[&str]) -> RemoteCommand {
        RemoteCommand {
            host: "localbox".to_string(),
            user: None,
            port: None,
            identity_file: None,
            known_hosts: None,
            tty: false,
            program: "echo".to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            setup: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    fn cmd(args: &[&str]) -> String {
        remote(args).command_line()
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

    #[test]
    fn the_command_is_the_last_argument() {
        let argv = remote(&["hi"]).to_ssh_args();

        assert_eq!(argv.last().unwrap(), "echo hi");
        assert_eq!(argv[argv.len() - 2], "localbox");
    }

    #[test]
    fn user_port_and_key_reach_ssh() {
        let mut command = remote(&["hi"]);
        command.user = Some("me".to_string());
        command.port = Some(2222);
        command.identity_file = Some(PathBuf::from("/keys/borrow"));
        command.known_hosts = Some(PathBuf::from("/keys/known_hosts"));

        let argv = command.to_ssh_args();

        assert!(argv.contains(&"me@localbox".to_string()), "argv was: {argv:?}");
        assert!(argv.windows(2).any(|w| w == ["-p", "2222"]), "argv was: {argv:?}");
        assert!(argv.windows(2).any(|w| w == ["-i", "/keys/borrow"]), "argv was: {argv:?}");
        assert!(
            argv.contains(&"UserKnownHostsFile=\"/keys/known_hosts\"".to_string()),
            "argv was: {argv:?}"
        );
    }

    /// ssh splits this option on whitespace because it can name several files.
    /// Without quotes a directory containing a space makes the box look unknown.
    /// ssh narrates its own lifecycle at INFO, which would put a closing notice
    /// after every single command. Errors stay visible at ERROR.
    #[test]
    fn ssh_is_told_to_keep_quiet_about_itself() {
        let argv = remote(&["hi"]).to_ssh_args();

        assert!(argv.contains(&"LogLevel=ERROR".to_string()), "argv was: {argv:?}");
    }

    #[test]
    fn a_terminal_is_requested_only_when_asked_for() {
        assert!(!remote(&["hi"]).to_ssh_args().contains(&"-t".to_string()));

        let mut interactive = remote(&["hi"]);
        interactive.tty = true;

        let argv = interactive.to_ssh_args();

        assert!(argv.contains(&"-t".to_string()), "argv was: {argv:?}");
        assert_eq!(argv.last().unwrap(), "echo hi");
    }

    #[test]
    fn a_known_hosts_path_with_a_space_stays_one_path() {
        let mut command = remote(&["hi"]);
        command.known_hosts = Some(PathBuf::from("/App Support/known_hosts"));

        let argv = command.to_ssh_args();

        assert!(
            argv.contains(&"UserKnownHostsFile=\"/App Support/known_hosts\"".to_string()),
            "argv was: {argv:?}"
        );
    }

    #[test]
    fn no_key_configured_means_no_i_flag() {
        let argv = remote(&["hi"]).to_ssh_args();

        assert!(!argv.contains(&"-i".to_string()), "argv was: {argv:?}");
    }

    #[test]
    fn a_working_directory_becomes_a_cd() {
        let mut command = remote(&["hi"]);
        command.cwd = Some("/mnt/borrow/app".to_string());

        assert_eq!(command.command_line(), "cd /mnt/borrow/app && echo hi");
    }

    /// The assignment is quoted whole rather than by halves, so a value is never
    /// able to end the word it is in.
    #[test]
    fn split_variables_are_put_in_front_of_the_command() {
        let mut command = remote(&["hi"]);
        command.env = vec![("CARGO_TARGET_DIR".to_string(), "/var/lib/b/target".to_string())];

        assert_eq!(
            command.command_line(),
            "env 'CARGO_TARGET_DIR=/var/lib/b/target' echo hi"
        );
    }

    /// The reason the parts are joined with && rather than ;. A failed mount must
    /// stop the build, not let it run somewhere empty and look like it worked.
    #[test]
    fn a_failed_setup_step_stops_the_command() {
        let mut command = remote(&["hi"]);
        command.setup = vec!["mount_it".to_string()];
        command.cwd = Some("/mnt/borrow/app".to_string());

        assert_eq!(command.command_line(), "mount_it && cd /mnt/borrow/app && echo hi");
    }

    #[test]
    fn a_project_path_with_a_space_stays_one_word() {
        let mut command = remote(&["hi"]);
        command.cwd = Some("/mnt/borrow/my app".to_string());

        assert_eq!(command.command_line(), "cd '/mnt/borrow/my app' && echo hi");
    }

    /// A variable's value is user controlled, so it is quoted like anything else.
    #[test]
    fn a_variable_value_cannot_break_out() {
        let mut command = remote(&["hi"]);
        command.env = vec![("K".to_string(), "a; rm -rf /".to_string())];

        assert_eq!(command.command_line(), "env 'K=a; rm -rf /' echo hi");
    }
}
