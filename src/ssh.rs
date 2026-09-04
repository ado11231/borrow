//! Building and running commands on the Agent over ssh.

use shell_words::join;

/// A command to run on the Agent. `cwd` and `env` are unused until for now
/// The mounted project path and the artifact split variables.
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