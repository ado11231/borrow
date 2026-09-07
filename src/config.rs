//! Where the Client remembers which Agent to talk to.

use crate::protocol::{Specs, DEFAULT_PORT};
use anyhow::Context;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Shown whenever there is no box to talk to. Names the two commands that fix it,
/// because a stranger has no config file to look at yet.
const NO_AGENTS: &str = "no agent configured yet\n\n\
                         on the box:      borrow serve\n\
                         on this machine: borrow link <code>";

/// The whole config file. `agents` is a list so it renders as readable `[[agents]]`
/// blocks for anyone who opens the file and edits it by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    default: Option<String>,
    #[serde(default)]
    agents: Vec<Agent>,
}

/// One box. `name` is the nickname the user types; `host` is what ssh dials.
/// `port` of `None` means 22, and `identity_file` of `None` falls back to the
/// user's own ssh setup rather than the key that pairing installed.
///
/// `specs` sits last because toml cannot put a plain value after a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    /// Where the daemon listens. Separate from `port`, which is for ssh.
    pub daemon_port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    /// The file holding this box's ssh host keys, learned at pairing.
    pub known_hosts: Option<PathBuf>,
    /// What the box is, fetched once at pairing so `info` is instant.
    pub specs: Option<Specs>,
}

impl Agent {
    /// The daemon port to dial, falling back to the built in default.
    pub fn daemon_port(&self) -> u16 {
        self.daemon_port.unwrap_or(DEFAULT_PORT)
    }
}

/// The platform's config directory for borrow. The only place that knows this
/// path, so no OS specific path appears anywhere else.
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

/// Where borrow keeps the host keys of the boxes it has paired with. Kept apart
/// from your own `~/.ssh/known_hosts` so borrow only ever edits its own files.
pub fn known_hosts_path() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("known_hosts"))
}

impl Config {
    /// Read and parse the config file. A missing file is the first run case, not a
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
    /// typo is a fix that takes a second.
    fn find(&self, name: &str) -> anyhow::Result<&Agent> {
        let names: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();

        self.agents
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no agent named '{name}'. configured agents: {}",
                    names.join(", ")
                )
            })
    }

    /// Pick the agent to use. An explicit `--agent` wins, then `default`, then the
    /// sole agent if there is only one.
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

    /// Read the config, treating a missing file as an empty one. Used by the
    /// commands that are about to write, where nothing saved yet is normal.
    pub fn load_or_empty() -> anyhow::Result<Config> {
        match path()?.exists() {
            true => Config::load(),
            false => Ok(Config { default: None, agents: Vec::new() }),
        }
    }

    /// Add a box, or replace the entry of the same name when pairing again. The
    /// first box paired becomes the default, so `run` works with no flags.
    pub fn upsert(&mut self, agent: Agent) {
        self.agents.retain(|a| a.name != agent.name);

        if self.default.is_none() && self.agents.is_empty() {
            self.default = Some(agent.name.clone());
        }

        self.agents.push(agent);
    }

    /// Forget a box. Clears the default too when it pointed at that box, so the
    /// config never names an agent that is not there.
    pub fn remove(&mut self, name: &str) -> anyhow::Result<Agent> {
        let Some(index) = self.agents.iter().position(|a| a.name == name) else {
            anyhow::bail!("no agent named '{name}'");
        };

        if self.default.as_deref() == Some(name) {
            self.default = None;
        }

        Ok(self.agents.remove(index))
    }

    /// Write the config back out, creating the directory the first time.
    pub fn save(&self) -> anyhow::Result<PathBuf> {
        let file = path()?;

        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }

        let body = toml::to_string_pretty(self).context("could not encode the config")?;
        fs::write(&file, body).with_context(|| format!("could not write {}", file.display()))?;

        Ok(file)
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

    fn agent(name: &str) -> Agent {
        Agent {
            name: name.to_string(),
            host: format!("{name}.local"),
            user: "me".to_string(),
            port: None,
            daemon_port: None,
            identity_file: None,
            known_hosts: None,
            specs: None,
        }
    }

    #[test]
    fn the_first_box_paired_becomes_the_default() {
        let mut config = config("");
        config.upsert(agent("archbox"));

        assert_eq!(config.default.as_deref(), Some("archbox"));
        assert_eq!(config.resolve(None).unwrap().name, "archbox");
    }

    #[test]
    fn pairing_again_replaces_rather_than_duplicates() {
        let mut config = config("");
        config.upsert(agent("archbox"));

        let mut moved = agent("archbox");
        moved.host = "10.0.0.9".to_string();
        config.upsert(moved);

        assert_eq!(config.agents.len(), 1);
        assert_eq!(config.agents[0].host, "10.0.0.9");
    }

    #[test]
    fn removing_the_default_box_clears_the_default() {
        let mut config = config(TWO);
        config.remove("laptop").unwrap();

        assert_eq!(config.default, None);
        assert!(config.remove("laptop").is_err());
    }

    #[test]
    fn a_box_with_cached_specs_survives_a_toml_round_trip() {
        let mut with_specs = agent("archbox");
        with_specs.daemon_port = Some(7433);
        with_specs.specs = Some(Specs {
            name: "archbox".to_string(),
            os: "Arch Linux".to_string(),
            kernel: "6.9.1".to_string(),
            cpu: "Ryzen 5".to_string(),
            cores: 12,
            memory_mb: 32000,
            disk_total_mb: 900000,
            tools: vec!["docker".to_string()],
            gpus: vec![crate::protocol::Gpu {
                name: "RTX 3070".to_string(),
                vram_mb: Some(8192),
            }],
        });

        let mut config = config("");
        config.upsert(with_specs);

        let text = toml::to_string_pretty(&config).expect("config should encode");
        let back: Config = toml::from_str(&text).expect("config should decode");

        assert_eq!(back.agents[0].specs.as_ref().unwrap().gpus[0].name, "RTX 3070");
        assert_eq!(back.agents[0].daemon_port(), 7433);
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
