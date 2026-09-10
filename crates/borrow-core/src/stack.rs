//! Project discovery from Rust, Node, Python, and Borrow marker files.

use std::path::{Path, PathBuf};

/// A supported stack with rules for local Agent build output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Rust,
    Node,
    Python,
}

/// Project markers. A borrow.toml file also marks a project without a known stack.
const MARKERS: &[(&str, Option<Stack>)] = &[
    ("borrow.toml", None),
    ("Cargo.toml", Some(Stack::Rust)),
    ("package.json", Some(Stack::Node)),
    ("pyproject.toml", Some(Stack::Python)),
    ("requirements.txt", Some(Stack::Python)),
];

/// A project root and its detected stacks. Mixed projects need every applicable split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub stacks: Vec<Stack>,
}

impl Project {
    /// Whether this project needs a given split.
    pub fn uses(&self, stack: Stack) -> bool {
        self.stacks.contains(&stack)
    }
}

/// Every stack with a marker file sitting in this directory, in a stable order and
/// with no repeats. Python has two markers and only wants counting once.
pub fn stacks_in(dir: &Path) -> Vec<Stack> {
    let mut found = Vec::new();

    for (marker, stack) in MARKERS {
        let Some(stack) = stack else { continue };

        if dir.join(marker).exists() && !found.contains(stack) {
            found.push(*stack);
        }
    }

    found
}

fn is_project_root(dir: &Path) -> bool {
    MARKERS.iter().any(|(marker, _)| dir.join(marker).exists())
}

/// Find the nearest project marker at or above start.
/// A workspace member is selected before its parent workspace.
pub fn find(start: &Path) -> Option<Project> {
    let mut dir = start;

    loop {
        if is_project_root(dir) {
            return Some(Project {
                root: dir.to_path_buf(),
                stacks: stacks_in(dir),
            });
        }

        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A throwaway directory tree. Tests touch the real filesystem because that is
    /// what detection does, and faking it would test the fake.
    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Sandbox {
            let root = std::env::temp_dir().join(format!("borrow-stack-{name}"));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("sandbox should be creatable");

            Sandbox { root }
        }

        fn dir(&self, path: &str) -> PathBuf {
            let dir = self.root.join(path);
            fs::create_dir_all(&dir).expect("directory should be creatable");
            dir
        }

        fn file(&self, path: &str) -> &Sandbox {
            let file = self.root.join(path);

            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent).expect("parent should be creatable");
            }

            fs::write(&file, "").expect("file should be writable");
            self
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_cargo_project_is_rust() {
        let sandbox = Sandbox::new("rust");
        sandbox.file("Cargo.toml");

        assert_eq!(stacks_in(&sandbox.root), vec![Stack::Rust]);
    }

    #[test]
    fn a_package_json_is_node() {
        let sandbox = Sandbox::new("node");
        sandbox.file("package.json");

        assert_eq!(stacks_in(&sandbox.root), vec![Stack::Node]);
    }

    #[test]
    fn either_python_marker_is_enough() {
        let modern = Sandbox::new("py-modern");
        modern.file("pyproject.toml");

        let older = Sandbox::new("py-older");
        older.file("requirements.txt");

        assert_eq!(stacks_in(&modern.root), vec![Stack::Python]);
        assert_eq!(stacks_in(&older.root), vec![Stack::Python]);
    }

    /// Both python markers in one project must not report python twice, or the
    /// split would be applied twice.
    #[test]
    fn two_python_markers_still_mean_one_stack() {
        let sandbox = Sandbox::new("py-both");
        sandbox.file("pyproject.toml").file("requirements.txt");

        assert_eq!(stacks_in(&sandbox.root), vec![Stack::Python]);
    }

    /// A tauri app, or a rust api with a node frontend. Missing the second stack
    /// would leave node_modules on the mount, which is the whole thing we are
    /// trying to avoid.
    #[test]
    fn a_polyglot_project_reports_every_stack() {
        let sandbox = Sandbox::new("polyglot");
        sandbox.file("Cargo.toml").file("package.json");

        let stacks = stacks_in(&sandbox.root);

        assert!(stacks.contains(&Stack::Rust), "stacks were: {stacks:?}");
        assert!(stacks.contains(&Stack::Node), "stacks were: {stacks:?}");
        assert_eq!(stacks.len(), 2);
    }

    #[test]
    fn an_empty_directory_has_no_stack() {
        let sandbox = Sandbox::new("empty");

        assert!(stacks_in(&sandbox.root).is_empty());
        assert_eq!(find(&sandbox.root), None);
    }

    #[test]
    fn the_root_is_found_from_a_directory_below_it() {
        let sandbox = Sandbox::new("nested");
        sandbox.file("Cargo.toml");
        let deep = sandbox.dir("src/commands/inner");

        let project = find(&deep).expect("should find the project above");

        assert_eq!(project.root, sandbox.root);
        assert!(project.uses(Stack::Rust));
    }

    /// A workspace member is the project. You typed the command in there, so that
    /// is the thing you meant.
    #[test]
    fn the_nearest_root_wins() {
        let sandbox = Sandbox::new("workspace");
        sandbox.file("Cargo.toml");
        sandbox.file("crates/inner/Cargo.toml");

        let project = find(&sandbox.root.join("crates/inner")).expect("should find inner");

        assert_eq!(project.root, sandbox.root.join("crates/inner"));
    }

    /// A borrow.toml marks a project even when nothing else does, so an override
    /// can name a stack borrow would not have detected.
    #[test]
    fn a_borrow_toml_alone_marks_a_project() {
        let sandbox = Sandbox::new("override");
        sandbox.file("borrow.toml");

        let project = find(&sandbox.root).expect("borrow.toml should mark a root");

        assert_eq!(project.root, sandbox.root);
        assert!(project.stacks.is_empty());
    }
}
