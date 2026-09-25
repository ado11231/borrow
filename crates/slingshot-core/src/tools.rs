//! The developer tools Slingshot can set up on the Agent. The Agent reports which it has,
//! and the Client decides what to offer and shows every command before anything runs.

use crate::preflight::install_command;
use crate::stack::Stack;
use crate::telemetry::is_installed;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Git,
    Docker,
    Node,
    Python,
    Rust,
    ClaudeCode,
    Codex,
}

/// Every tool, in install order. Codex installs with npm, so Node comes before it.
pub const ALL: [Tool; 7] = [
    Tool::Git,
    Tool::Docker,
    Tool::Node,
    Tool::Python,
    Tool::Rust,
    Tool::ClaudeCode,
    Tool::Codex,
];

/// How to sign in to a tool on the Agent. Credentials are never copied from the Client,
/// so each tool signs in where it runs.
#[derive(Debug, PartialEq, Eq)]
pub struct SignIn {
    pub args: &'static [&'static str],
    /// A port on the Agent that the sign in page on the Client must reach, forwarded over ssh.
    pub forward: Option<u16>,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Git => "Git",
            Tool::Docker => "Docker",
            Tool::Node => "Node and npm",
            Tool::Python => "Python",
            Tool::Rust => "Rust",
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
        }
    }

    /// The program whose presence shows the tool is installed.
    pub fn program(self) -> &'static str {
        match self {
            Tool::Git => "git",
            Tool::Docker => "docker",
            Tool::Node => "node",
            Tool::Python => "python3",
            Tool::Rust => "cargo",
            Tool::ClaudeCode => "claude",
            Tool::Codex => "codex",
        }
    }

    fn needs(self) -> Option<Tool> {
        match self {
            Tool::Codex => Some(Tool::Node),
            _ => None,
        }
    }

    fn for_stack(stack: Stack) -> Tool {
        match stack {
            Stack::Rust => Tool::Rust,
            Stack::Node => Tool::Node,
            Stack::Python => Tool::Python,
        }
    }

    /// Packages for tools that come from the system package manager.
    fn packages(self, manager: &str) -> Option<&'static str> {
        match (self, manager) {
            (Tool::Git, _) => Some("git"),
            (Tool::Docker, "pacman" | "zypper") => Some("docker"),
            (Tool::Docker, "apt") => Some("docker.io"),
            (Tool::Docker, "dnf") => Some("moby-engine"),
            (Tool::Node, "brew") => Some("node"),
            (Tool::Node, "pacman" | "apt" | "dnf" | "apk") => Some("nodejs npm"),
            (Tool::Python, "pacman") => Some("python python-pip"),
            (Tool::Python, "apt") => Some("python3 python3-pip python3-venv"),
            (Tool::Python, "dnf" | "zypper") => Some("python3 python3-pip"),
            (Tool::Python, "apk") => Some("python3 py3-pip"),
            (Tool::Python, "brew") => Some("python"),
            _ => None,
        }
    }

    /// The commands that install this tool on an Agent using `manager`. `None` means
    /// Slingshot has no command it trusts there, so the person installs it themselves.
    pub fn install(self, manager: Option<&str>) -> Option<Vec<String>> {
        match self {
            Tool::Rust => Some(vec![
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y".into(),
            ]),
            Tool::ClaudeCode => Some(vec!["curl -fsSL https://claude.ai/install.sh | bash".into()]),
            Tool::Codex => Some(vec![
                "if [ -w \"$(npm prefix -g)\" ]; then npm install -g @openai/codex; else sudo npm install -g @openai/codex; fi".into(),
            ]),
            Tool::Docker => {
                let manager = manager?;
                let mut commands = vec![install_command(manager, self.packages(manager)?)?];
                commands.push("sudo systemctl enable --now docker".into());
                commands.push("sudo usermod -aG docker \"$USER\"".into());
                Some(commands)
            }
            Tool::Git | Tool::Node | Tool::Python => {
                let manager = manager?;
                Some(vec![install_command(manager, self.packages(manager)?)?])
            }
        }
    }

    pub fn sign_in(self) -> Option<SignIn> {
        match self {
            Tool::ClaudeCode => Some(SignIn {
                args: &["auth", "login"],
                forward: None,
            }),
            Tool::Codex => Some(SignIn {
                args: &["login"],
                forward: Some(1455),
            }),
            _ => None,
        }
    }
}

/// What to offer the Agent: tools the Client has or the project needs, plus anything those
/// need to install, minus what the Agent already has. Returned in install order.
pub fn missing(client: &[Tool], agent: &[Tool], stacks: &[Stack]) -> Vec<Tool> {
    let mut wanted: Vec<Tool> = client
        .iter()
        .copied()
        .chain(stacks.iter().map(|stack| Tool::for_stack(*stack)))
        .collect();
    let needed: Vec<Tool> = wanted.iter().filter_map(|tool| tool.needs()).collect();
    wanted.extend(needed);
    ALL.into_iter()
        .filter(|tool| wanted.contains(tool) && !agent.contains(tool))
        .collect()
}

/// The tools installed on this machine, as the current PATH sees them.
pub fn installed_here() -> Vec<Tool> {
    ALL.into_iter()
        .filter(|tool| is_installed(tool.program()))
        .collect()
}

/// Folders in the home folder where installers put programs for one user, such as Claude
/// Code in `~/.local/bin`. Login shells that are not interactive often leave them off PATH,
/// so Slingshot adds them to everything it starts on the Agent instead of editing any file.
pub const USER_FOLDERS: [&str; 2] = [".local/bin", ".cargo/bin"];

/// A shell line that puts `USER_FOLDERS` at the front of PATH.
pub fn user_path_line() -> String {
    let folders: Vec<String> = USER_FOLDERS
        .iter()
        .map(|folder| format!("$HOME/{folder}"))
        .collect();
    format!("export PATH=\"{}:$PATH\"", folders.join(":"))
}

/// `path` with each folder of `USER_FOLDERS` under `home` added in front, unless it is
/// already there, for programs started without a shell.
pub fn with_user_folders(path: &str, home: &Path) -> String {
    let current: Vec<&str> = path.split(':').filter(|part| !part.is_empty()).collect();
    let added: Vec<String> = USER_FOLDERS
        .iter()
        .map(|folder| home.join(folder).display().to_string())
        .filter(|folder| !current.contains(&folder.as_str()))
        .collect();
    added
        .iter()
        .map(String::as_str)
        .chain(current)
        .collect::<Vec<&str>>()
        .join(":")
}

/// A script that prints the program name of each tool it finds. The Agent runs it in a
/// login shell with the per user folders added, so it sees what a session and a run do.
pub fn probe_script() -> String {
    let programs: Vec<&str> = ALL.iter().map(|tool| tool.program()).collect();
    format!(
        "{}; for program in {}; do if command -v \"$program\" >/dev/null 2>&1; then echo \"$program\"; fi; done",
        user_path_line(),
        programs.join(" ")
    )
}

/// The tools named in the output of `probe_script`.
pub fn found(output: &str) -> Vec<Tool> {
    let programs: Vec<&str> = output.lines().map(str::trim).collect();
    ALL.into_iter()
        .filter(|tool| programs.contains(&tool.program()))
        .collect()
}

/// One script that installs `tools` in order and names each before it starts. It keeps
/// going after a failure, because the Agent is checked again afterwards to see what worked.
/// Tools without a command are left out.
pub fn install_script(tools: &[Tool], manager: Option<&str>) -> String {
    let mut lines = Vec::new();
    for tool in tools {
        let Some(commands) = tool.install(manager) else {
            continue;
        };
        lines.push(format!("echo; echo 'Installing {}'", tool.name()));
        lines.extend(commands);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_the_client_has_and_the_agent_lacks_are_offered() {
        let offered = missing(&[Tool::Git, Tool::Docker], &[Tool::Git], &[]);

        assert_eq!(offered, vec![Tool::Docker]);
    }

    #[test]
    fn the_project_adds_its_own_tools() {
        let offered = missing(&[], &[], &[Stack::Rust, Stack::Python]);

        assert_eq!(offered, vec![Tool::Python, Tool::Rust]);
    }

    #[test]
    fn codex_brings_node_when_the_agent_has_none() {
        assert_eq!(
            missing(&[Tool::Codex], &[], &[]),
            vec![Tool::Node, Tool::Codex]
        );
        assert_eq!(
            missing(&[Tool::Codex], &[Tool::Node], &[]),
            vec![Tool::Codex]
        );
    }

    #[test]
    fn nothing_is_offered_when_the_agent_has_everything() {
        assert!(missing(&ALL, &ALL, &[Stack::Node]).is_empty());
    }

    #[test]
    fn offers_come_in_install_order_without_repeats() {
        let offered = missing(&[Tool::Codex, Tool::Git, Tool::Node], &[], &[Stack::Node]);

        assert_eq!(offered, vec![Tool::Git, Tool::Node, Tool::Codex]);
    }

    #[test]
    fn docker_on_linux_also_starts_the_service_and_joins_the_group() {
        let commands = Tool::Docker.install(Some("pacman")).unwrap();

        assert_eq!(commands[0], "sudo pacman -S docker");
        assert!(
            commands
                .iter()
                .any(|c| c.contains("systemctl enable --now docker"))
        );
        assert!(commands.iter().any(|c| c.contains("usermod -aG docker")));
        assert_eq!(
            Tool::Docker.install(Some("apt")).unwrap()[0],
            "sudo apt install docker.io"
        );
    }

    #[test]
    fn tools_without_a_trusted_command_are_left_to_the_person() {
        assert_eq!(Tool::Docker.install(Some("brew")), None);
        assert_eq!(Tool::Git.install(None), None);
        assert_eq!(Tool::Node.install(Some("zypper")), None);
    }

    #[test]
    fn installers_that_need_no_package_manager_always_have_a_command() {
        for tool in [Tool::Rust, Tool::ClaudeCode, Tool::Codex] {
            assert!(tool.install(None).is_some(), "{tool:?}");
        }
    }

    #[test]
    fn the_install_script_names_each_tool_and_skips_ones_without_a_command() {
        let script = install_script(&[Tool::Git, Tool::Docker, Tool::Rust], Some("brew"));

        assert!(script.contains("echo 'Installing Git'\nbrew install git"));
        assert!(!script.contains("Docker"));
        assert!(script.contains("Installing Rust"));
    }

    #[test]
    fn user_folders_go_in_front_once() {
        let home = Path::new("/home/me");

        assert_eq!(
            with_user_folders("/usr/local/bin:/usr/bin", home),
            "/home/me/.local/bin:/home/me/.cargo/bin:/usr/local/bin:/usr/bin"
        );
        assert_eq!(
            with_user_folders("/home/me/.cargo/bin:/usr/bin", home),
            "/home/me/.local/bin:/home/me/.cargo/bin:/usr/bin"
        );
        assert_eq!(
            with_user_folders("", home),
            "/home/me/.local/bin:/home/me/.cargo/bin"
        );
    }

    #[test]
    fn the_probe_looks_in_the_user_folders_first() {
        assert!(
            probe_script().starts_with("export PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\";")
        );
    }

    #[test]
    fn probe_output_maps_back_to_tools() {
        assert!(probe_script().contains("claude"));
        assert_eq!(found("git\ncodex\nunknown\n"), vec![Tool::Git, Tool::Codex]);
    }

    #[test]
    fn only_coding_agents_sign_in() {
        assert_eq!(Tool::Codex.sign_in().unwrap().forward, Some(1455));
        assert_eq!(Tool::ClaudeCode.sign_in().unwrap().forward, None);
        assert_eq!(Tool::Git.sign_in(), None);
    }
}
