//! Where the Client remembers which Agent to talk to.

use anyhow::Context;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Shown whenever there is no box to talk to. Names the two commands that fix it,
/// because a stranger has no config file to inspect yet.
const NO_AGENTS: &str = "no agent configured yet\n\n\
                         on the box:      borrow serve\n\
                         on this machine: borrow link <code>";

/// The whole config file. `agents` is a list so it renders as readable `[[agents]]`
/// blocks for anyone who hand-edits it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    default: Option<String>,
    #[serde(default)]
    agents: Vec<Agent>,
}

/// One box. `name` is the nickname the user types; `host` is what ssh dials.
/// `port` of `None` means 22, and `identity_file` of `None` falls back to the
/// user's own ssh setup until pairing installs a key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

/// The platform's config directory for borrow. The only place that knows this
/// path, so no OS-specific path appears anywhere else.
fn dir() -> anyhow::Result<PathBuf> {
    let Some(dirs) = ProjectDirs::from("", "", "borrow") else {
        anyhow::bail!("Could not determine home directory");
    };
    Ok(dirs.config_dir().to_path_buf())
}

/// Full path to the config file.
pub fn path() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("config.toml"))
}

impl Config {
    /// Read and parse the config file. A missing file is the first-run case, not a
    /// filesystem error, so it reports the pairing commands instead.
    pub fn load() -> anyhow::Result<Config> {
        let file = path()?;

        if !file.exists() {
            anyhow::bail!(NO_AGENTS);
        }

        let content = fs::read_to_string(&file)
            .with_context(|| format!("Could not read {}", file.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Could not parse {}", file.display()))?;

        Ok(config)
    }

    /// Look up one agent by nickname. A miss lists the names that do exist, so a
    /// typo is a one-second fix.
    fn find(&self, name: &str) -> anyhow::Result<&Agent> {
        let names: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();

        self.agents
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no agent named '{name}' — configured agents: {}",
                    names.join(", ")
                )
            })
    }

    /// Pick the agent to use. An explicit `--agent` wins, then `default`, then the
    /// sole agent if there is only one. A single-box user never has to learn that
    /// defaults exist.
    pub fn resolve(&self, requested: Option<&str>) -> anyhow::Result<&Agent> {
        match (requested, self.default.as_deref()) {
            (Some(name), _) => self.find(name),
            (None, Some(default)) => self.find(default),
            (None, None) => match self.agents.as_slice() {
                [] => anyhow::bail!(NO_AGENTS),
                [only] => Ok(only),
                _ => {
                    let names: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();
                    anyhow::bail!(
                        "several agents configured: {}\n\n\
                         pass --agent <name>, or set default = \"<name>\" in {}",
                        names.join(", "),
                        path()?.display()
                    )
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = r#"
        default = "laptop"

        [[agents]]
        name = "archbox"
        host = "archbox.local"
        user = "me"

        [[agents]]
        name = "laptop"
        host = "laptop.local"
        user = "me"
    "#;

    const ONE: &str = r#"
        [[agents]]
        name = "archbox"
        host = "archbox.local"
        user = "me"
    "#;

    fn config(toml: &str) -> Config {
        toml::from_str(toml).expect("Test fixture should parse")
    }

    #[test]
    fn default_is_used_when_no_agent_requested() {
        assert_eq!(config(TWO).resolve(None).unwrap().name, "laptop");
    }

    #[test]
    fn explicit_request_beats_the_default() {
        assert_eq!(config(TWO).resolve(Some("archbox")).unwrap().name, "archbox");
    }

    #[test]
    fn sole_agent_is_used_without_a_default() {
        assert_eq!(config(ONE).resolve(None).unwrap().name, "archbox");
    }

    #[test]
    fn unknown_agent_lists_what_exists() {
        let err = config(TWO).resolve(Some("nope")).unwrap_err().to_string();

        assert!(err.contains("nope"), "message was: {err}");
        assert!(err.contains("archbox"), "message was: {err}");
        assert!(err.contains("laptop"), "message was: {err}");
    }

    #[test]
    fn default_naming_a_missing_agent_is_reported() {
        let err = config(
            r#"
            default = "ghost"

            [[agents]]
            name = "archbox"
            host = "archbox.local"
            user = "me"
            "#,
        )
        .resolve(None)
        .unwrap_err()
        .to_string();

        assert!(err.contains("ghost"), "message was: {err}");
    }

    #[test]
    fn several_agents_without_a_default_is_ambiguous() {
        let err = config(
            r#"
            [[agents]]
            name = "archbox"
            host = "archbox.local"
            user = "me"

            [[agents]]
            name = "laptop"
            host = "laptop.local"
            user = "me"
            "#,
        )
        .resolve(None)
        .unwrap_err()
        .to_string();

        assert!(err.contains("--agent"), "message was: {err}");
        assert!(err.contains("archbox"), "message was: {err}");
    }

    #[test]
    fn no_agents_points_at_pairing() {
        let err = config("").resolve(None).unwrap_err().to_string();

        assert!(err.contains("borrow link"), "message was: {err}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let result = toml::from_str::<Config>(
            r#"
            [[agents]]
            name = "archbox"
            hostname = "archbox.local"
            user = "me"
            "#,
        );

        assert!(result.is_err());
    }
}
