//! Working out what kind of project you are in.
//!
//! This is what makes zero configuration possible. Detect the stack, and the
//! artifact split follows from it. Detection is deliberately dumb: look for the
//! file that defines each ecosystem, and never guess from anything else.

use std::path::{Path, PathBuf};

/// A stack borrow knows how to keep build output off the mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Rust,
    Node,
    Python,
}

/// The file that says "a project of this kind lives here". `borrow.toml` is listed
/// too, so a project borrow has been told about is found even when it looks like
/// nothing in particular.
const MARKERS: &[(&str, Option<Stack>)] = &[
    ("borrow.toml", None),
    ("Cargo.toml", Some(Stack::Rust)),
    ("package.json", Some(Stack::Node)),
    ("pyproject.toml", Some(Stack::Python)),
    ("requirements.txt", Some(Stack::Python)),
];

/// A project directory and what it is built with. `stacks` is a list because plenty
/// of real projects are more than one thing, and a Rust binary with a Node frontend
/// needs both splits or the one you missed lands on the mount.
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

/// True when a directory looks like the top of a project.
fn is_project_root(dir: &Path) -> bool {
    MARKERS.iter().any(|(marker, _)| dir.join(marker).exists())
}

/// Walk up from `start` until a directory looks like a project root.
///
/// Walking up is the point: you run `borrow run cargo build` from wherever you
/// happen to be, and the project is usually somewhere above you. The first match
/// wins, so a workspace member is treated as the project rather than the workspace,
/// which is what you meant when you typed the command in there.
pub fn find(start: &Path) -> Option<Project> {
    let mut dir = start;

    loop {
        if is_project_root(dir) {
            return Some(Project { root: dir.to_path_buf(), stacks: stacks_in(dir) });
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
