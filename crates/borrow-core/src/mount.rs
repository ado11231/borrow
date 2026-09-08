//! Where a project appears on the Agent, and where its build output goes instead.
//!
//! This is the make or break of the whole tool. Source travels over the mount, and
//! build output must not, because a compiler writes thousands of small files and
//! every one of them over a network filesystem turns a fast build into a painful
//! one. Getting this wrong does not make borrow slower, it makes borrow pointless.

use crate::stack::{Project, Stack};
use std::path::{Path, PathBuf};

/// Where the Client's project is mounted on the Agent.
pub const MOUNT_BASE: &str = "/mnt/borrow";

/// Where build output lives instead, on the Agent's own disk. Never on the mount.
pub const ARTIFACT_BASE: &str = "/var/lib/borrow/builds";

/// The two directories a project has on the Agent: the mounted source, and the
/// local disk its build output is pushed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub source: PathBuf,
    pub artifacts: PathBuf,
}

impl Layout {
    /// The layout for a project, using the standard directories.
    pub fn for_project(project: &Project) -> Layout {
        Layout::with_bases(project, Path::new(MOUNT_BASE), Path::new(ARTIFACT_BASE))
    }

    /// The same, with the bases named. Tests use this, and so will a future
    /// setting for people whose Agent keeps its fast disk somewhere else.
    pub fn with_bases(project: &Project, mount_base: &Path, artifact_base: &Path) -> Layout {
        let name = project
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        Layout {
            source: mount_base.join(&name),
            artifacts: artifact_base.join(&name),
            name,
        }
    }
}

/// One instruction for keeping build output off the mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// Set an environment variable and the tool writes elsewhere by itself. The
    /// tidiest kind, because nothing appears in the project at all.
    Env { key: &'static str, value: PathBuf },

    /// A directory the tool insists on finding inside the project. Nothing can talk
    /// it out of the location, so the Agent puts a link there pointing at local
    /// disk. Every one of these is gitignored in practice, which is what makes a
    /// link in your source tree tolerable.
    Redirect { name: &'static str, target: PathBuf },
}

/// What to do for this project. A project with several stacks gets the rules for
/// all of them, because missing one leaves that stack's output on the mount.
pub fn rules(project: &Project, layout: &Layout) -> Vec<Rule> {
    let mut rules = Vec::new();

    for stack in &project.stacks {
        match stack {
            Stack::Rust => rules.push(Rule::Env {
                key: "CARGO_TARGET_DIR",
                value: layout.artifacts.join("target"),
            }),

            Stack::Node => rules.push(Rule::Redirect {
                name: "node_modules",
                target: layout.artifacts.join("node_modules"),
            }),

            Stack::Python => {
                rules.push(Rule::Env {
                    key: "PIP_CACHE_DIR",
                    value: layout.artifacts.join("pip-cache"),
                });
                rules.push(Rule::Redirect {
                    name: ".venv",
                    target: layout.artifacts.join("venv"),
                });
            }
        }
    }

    rules
}

/// The environment variables to put in front of the remote command.
pub fn env(rules: &[Rule]) -> Vec<(String, String)> {
    rules
        .iter()
        .filter_map(|rule| match rule {
            Rule::Env { key, value } => {
                Some((key.to_string(), value.display().to_string()))
            }
            Rule::Redirect { .. } => None,
        })
        .collect()
}

/// The directories the Agent has to create and link before the command runs.
pub fn redirects(rules: &[Rule]) -> Vec<(&'static str, &Path)> {
    rules
        .iter()
        .filter_map(|rule| match rule {
            Rule::Redirect { name, target } => Some((*name, target.as_path())),
            Rule::Env { .. } => None,
        })
        .collect()
}

/// A short phrase for the line borrow prints before it runs anything, so you can
/// see the split happened rather than trusting that it did.
pub fn summary(rules: &[Rule]) -> Option<String> {
    let names: Vec<&str> = rules
        .iter()
        .map(|rule| match rule {
            Rule::Env { key, .. } if *key == "CARGO_TARGET_DIR" => "target",
            Rule::Env { key, .. } if *key == "PIP_CACHE_DIR" => "pip cache",
            Rule::Env { key, .. } => key,
            Rule::Redirect { name, .. } => name,
        })
        .collect();

    match names.is_empty() {
        true => None,
        false => Some(format!("{} → local disk", names.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(stacks: Vec<Stack>) -> Project {
        Project { root: PathBuf::from("/Users/me/projects/app"), stacks }
    }

    fn layout_for(project: &Project) -> Layout {
        Layout::for_project(project)
    }

    #[test]
    fn a_project_is_mounted_under_its_own_name() {
        let layout = layout_for(&project(vec![Stack::Rust]));

        assert_eq!(layout.name, "app");
        assert_eq!(layout.source, PathBuf::from("/mnt/borrow/app"));
        assert_eq!(layout.artifacts, PathBuf::from("/var/lib/borrow/builds/app"));
    }

    #[test]
    fn rust_redirects_the_target_directory() {
        let project = project(vec![Stack::Rust]);
        let rules = rules(&project, &layout_for(&project));

        assert_eq!(
            env(&rules),
            vec![(
                "CARGO_TARGET_DIR".to_string(),
                "/var/lib/borrow/builds/app/target".to_string()
            )]
        );
        assert!(redirects(&rules).is_empty());
    }

    /// npm has no environment variable for this, so a link is the only way.
    #[test]
    fn node_needs_a_link_because_npm_has_no_setting() {
        let project = project(vec![Stack::Node]);
        let rules = rules(&project, &layout_for(&project));

        assert!(env(&rules).is_empty());
        assert_eq!(
            redirects(&rules),
            vec![("node_modules", Path::new("/var/lib/borrow/builds/app/node_modules"))]
        );
    }

    #[test]
    fn python_needs_both_a_variable_and_a_link() {
        let project = project(vec![Stack::Python]);
        let rules = rules(&project, &layout_for(&project));

        assert_eq!(env(&rules).len(), 1);
        assert_eq!(redirects(&rules).len(), 1);
    }

    #[test]
    fn a_polyglot_project_gets_every_rule() {
        let project = project(vec![Stack::Rust, Stack::Node]);
        let rules = rules(&project, &layout_for(&project));

        assert_eq!(env(&rules).len(), 1);
        assert_eq!(redirects(&rules).len(), 1);
    }

    /// The one rule that matters. If any artifact path ever lands under the mount,
    /// the build writes over the network and the tool is pointless.
    #[test]
    fn no_rule_ever_points_inside_the_mount() {
        let project = project(vec![Stack::Rust, Stack::Node, Stack::Python]);
        let layout = layout_for(&project);
        let rules = rules(&project, &layout);

        for (key, value) in env(&rules) {
            assert!(
                !Path::new(&value).starts_with(&layout.source),
                "{key} points at {value}, which is on the mount"
            );
        }

        for (name, target) in redirects(&rules) {
            assert!(
                !target.starts_with(&layout.source),
                "{name} points at {}, which is on the mount",
                target.display()
            );
        }
    }

    #[test]
    fn a_project_with_no_known_stack_gets_no_rules() {
        let project = project(Vec::new());
        let rules = rules(&project, &layout_for(&project));

        assert!(rules.is_empty());
        assert_eq!(summary(&rules), None);
    }

    #[test]
    fn the_summary_names_what_was_moved() {
        let project = project(vec![Stack::Rust, Stack::Node]);
        let rules = rules(&project, &layout_for(&project));

        assert_eq!(summary(&rules), Some("target, node_modules → local disk".to_string()));
    }
}
