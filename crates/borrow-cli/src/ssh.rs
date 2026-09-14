//! Building and running SSH commands to the Agent.

use anyhow::Context;
use borrow_core::config;
use shell_words::join;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// The exit code reported when SSH ends without a remote exit status.
const EXIT_SIGNALLED: i32 = 130;

/// Disable password prompts, keep idle connections alive, and show SSH errors only.
const SSH_OPTIONS: &[&str] = &[
    "BatchMode=yes",
    "ConnectTimeout=10",
    "ServerAliveInterval=30",
    "ServerAliveCountMax=6",
    "LogLevel=ERROR",
];

/// Request a terminal only for interactive use.
/// A terminal lets Ctrl C reach remote work but merges stdout and stderr.
pub fn wants_terminal() -> bool {
    use std::io::IsTerminal;

    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// A command to run on the Agent. Arguments are quoted for the remote shell, so no
/// value can ever be read as shell syntax.
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
        }
    }

    /// Who to log in as and where, in the form ssh expects.
    fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    pub fn command_line(&self) -> String {
        join(std::iter::once(self.program.as_str()).chain(self.args.iter().map(|s| s.as_str())))
    }

    /// SSH options and the destination, without any remote command.
    pub fn connection_args(&self) -> Vec<String> {
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
        argv
    }

    /// The full argument list for `ssh`. Options first, then the destination, then
    /// the command, because ssh treats everything after the destination as payload.
    pub fn to_ssh_args(&self) -> Vec<String> {
        let mut argv = self.connection_args();
        argv.push(self.command_line());
        argv
    }

    /// Run with this terminal's input and output attached directly, so bytes, window
    /// size changes, and Ctrl C pass through unchanged. Returns the remote exit code.
    /// SSH's own messages go to a private log instead of the terminal, so a dropped
    /// connection becomes a `Disconnected` error and anything else is shown as before.
    pub async fn interactive(&self) -> anyhow::Result<i32> {
        let log = SshLog::create()?;
        let mut child = Command::new("ssh")
            .arg("-E")
            .arg(&log.0)
            .args(self.to_ssh_args())
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Could not start ssh. Check that it is installed and on PATH")?;

        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.context("Waiting for ssh failed")?,
                _ = &mut interrupt => {}
            }
        };

        let code = status.code().unwrap_or(EXIT_SIGNALLED);
        let messages = log.read();
        match (code, messages.is_empty()) {
            (EXIT_SSH_FAILED, false) if connection_lost(&messages) => {
                Err(Disconnected::Certain.into())
            }
            (EXIT_SSH_FAILED, true) => Err(Disconnected::Possible.into()),
            _ => {
                eprint!("{messages}");
                Ok(code)
            }
        }
    }
}

impl RemoteCommand {
    /// Whether SSH can reach the Agent right now, without involving the daemon.
    pub async fn reachable(agent: &config::Agent) -> bool {
        let mut probe = RemoteCommand::to(agent, "true".to_string(), Vec::new());
        probe.tty = false;
        let status = Command::new("ssh")
            .args(probe.to_ssh_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status();
        matches!(
            tokio::time::timeout(std::time::Duration::from_secs(15), status).await,
            Ok(Ok(status)) if status.success()
        )
    }
}

/// The exit code SSH uses for its own failures. A remote command can exit with it too,
/// which is why the log decides whether SSH failed.
const EXIT_SSH_FAILED: i32 = 255;

/// SSH ended with its failure code. `Certain` means its messages describe a dropped
/// connection. `Possible` means it printed nothing, which is both how a keepalive
/// timeout ends and how a remote command exiting with 255 ends.
#[derive(Debug, PartialEq)]
pub enum Disconnected {
    Certain,
    Possible,
}

impl std::fmt::Display for Disconnected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Disconnected::Certain => f.write_str("The SSH connection to the Agent was lost"),
            Disconnected::Possible => f.write_str("SSH exited with code 255"),
        }
    }
}

impl std::error::Error for Disconnected {}

/// True when SSH's messages describe an established connection that dropped, as opposed
/// to one that never opened, which keeps its own more specific message.
fn connection_lost(messages: &str) -> bool {
    const NEVER_OPENED: &[&str] = &[
        "connect to host",
        "Could not resolve",
        "kex_exchange_identification",
        "banner exchange",
        "Permission denied",
        "Host key verification failed",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
    ];
    const DROPPED: &[&str] = &[
        "client_loop",
        "Broken pipe",
        "not responding",
        "closed by remote host",
        "Connection reset",
        "Read from remote host",
        "packet_write_wait",
        "Software caused connection abort",
    ];
    !NEVER_OPENED.iter().any(|marker| messages.contains(marker))
        && DROPPED.iter().any(|marker| messages.contains(marker))
}

/// A private file for SSH's `-E` log, removed when dropped. SSH closes inherited file
/// descriptors at startup, so a pipe cannot be used here.
struct SshLog(PathBuf);

impl SshLog {
    fn create() -> anyhow::Result<SshLog> {
        use std::os::unix::fs::OpenOptionsExt;

        let path =
            std::env::temp_dir().join(format!("borrow-ssh-{}.log", borrow_core::storage::new_id()));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .context("Could not create a temporary file for SSH messages")?;
        Ok(SshLog(path))
    }

    fn read(&self) -> String {
        std::fs::read(&self.0)
            .map(|bytes| String::from_utf8_lossy(&bytes).replace('\r', ""))
            .unwrap_or_default()
    }
}

impl Drop for SshLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The `-e` helper rsync starts: SSH with Borrow's saved options for one Agent. Running
/// through Borrow avoids passing file paths inside rsync's own option parsing.
pub fn exec_for_rsync(agent: &config::Agent, args: Vec<String>) -> anyhow::Error {
    use std::os::unix::process::CommandExt;

    let mut rest = args.into_iter().peekable();
    if rest.peek().is_some_and(|arg| arg == "-l") {
        rest.next();
        rest.next();
    }
    rest.next();
    let mut remote = RemoteCommand::to(agent, String::new(), Vec::new());
    remote.tty = false;
    let error = std::process::Command::new("ssh")
        .args(remote.connection_args())
        .args(rest)
        .exec();
    anyhow::Error::from(error).context("Could not start ssh")
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
    fn a_program_path_with_a_space_stays_one_word() {
        let mut command = remote(&["internal-control"]);
        command.program = "/Users/me/Application Support/borrow".to_string();
        assert_eq!(
            command.command_line(),
            "'/Users/me/Application Support/borrow' internal-control"
        );
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

        assert!(
            argv.contains(&"me@localbox".to_string()),
            "argv was: {argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w == ["-p", "2222"]),
            "argv was: {argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w == ["-i", "/keys/borrow"]),
            "argv was: {argv:?}"
        );
        assert!(
            argv.contains(&"UserKnownHostsFile=\"/keys/known_hosts\"".to_string()),
            "argv was: {argv:?}"
        );
    }

    #[test]
    fn ssh_is_told_to_keep_quiet_about_itself() {
        let argv = remote(&["hi"]).to_ssh_args();

        assert!(
            argv.contains(&"LogLevel=ERROR".to_string()),
            "argv was: {argv:?}"
        );
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

    /// Quote the known hosts path because SSH splits this option on whitespace.
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
    fn a_dropped_connection_is_told_apart_from_one_that_never_opened() {
        assert!(connection_lost(
            "Read from remote host 10.0.0.193: Can't assign requested address\nclient_loop: send disconnect: Broken pipe\n"
        ));
        assert!(connection_lost(
            "Timeout, server 10.0.0.193 not responding.\n"
        ));
        assert!(!connection_lost(
            "ssh: connect to host 10.0.0.193 port 22: Operation timed out\n"
        ));
        assert!(!connection_lost(
            "kex_exchange_identification: read: Connection reset by peer\n"
        ));
        assert!(!connection_lost(
            "ado@archbox: Permission denied (publickey).\n"
        ));
        assert!(!connection_lost(""));
    }

    #[test]
    fn the_ssh_log_is_private_and_removed_afterwards() {
        use std::os::unix::fs::PermissionsExt;

        let log = SshLog::create().unwrap();
        let path = log.0.clone();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::write(&path, "client_loop: send disconnect: Broken pipe\r\n").unwrap();
        assert_eq!(log.read(), "client_loop: send disconnect: Broken pipe\n");
        drop(log);
        assert!(!path.exists());
    }
}
