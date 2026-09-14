//! Which project files count as source, and how their content is fingerprinted.
//!
//! Git ignore rules, version control internals, generated folders, and environment
//! files never enter a manifest, so they are never copied, pulled, previewed, or
//! backed up. Environment files are handled separately by `borrow env`.

use crate::storage;
use anyhow::{Context, bail, ensure};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// One source file. A symlink stores its target text and hashes that text.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub hash: String,
    pub executable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// Every eligible file, keyed by its relative path with `/` separators.
pub type Manifest = BTreeMap<String, Entry>;

/// Folder names that are always generated or internal, at any depth.
const GENERATED: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".jj",
    "node_modules",
    ".venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".gradle",
    ".cache",
];

/// Prefix of Borrow's own temporary files, which must never be mistaken for source.
pub const PARTIAL_PREFIX: &str = ".borrow-partial-";

/// `.env`, `.env.local`, `.env.example`, `prod.env`, and `.envrc`, including templates.
pub fn is_environment_name(name: &str) -> bool {
    name == ".env" || name.starts_with(".env.") || name.ends_with(".env") || name == ".envrc"
}

/// The exclusions no setting can override. `target` counts as generated only beside a
/// `Cargo.toml`, so a source folder that happens to be called target is still copied.
pub fn mandatory(path: &str, has_file: impl Fn(&str) -> bool) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.iter().enumerate().any(|(index, name)| {
        if is_environment_name(name) || GENERATED.contains(name) || name.starts_with(PARTIAL_PREFIX)
        {
            return true;
        }
        if *name == "target" {
            let parent = parts[..index].join("/");
            let cargo = match parent.is_empty() {
                true => "Cargo.toml".to_string(),
                false => format!("{parent}/Cargo.toml"),
            };
            return has_file(&cargo);
        }
        false
    })
}

/// Extra exclusion patterns for a project, gathered on the Client and sent to the Agent
/// so both machines apply exactly the same rules. Repository and global git excludes
/// come first and `borrow.toml` patterns last. Every pattern is anchored at the root.
pub fn client_excludes(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut patterns = Vec::new();
    patterns.extend(pattern_lines(&root.join(".git/info/exclude")));
    if let Some(global) = global_excludes_file(root) {
        patterns.extend(pattern_lines(&global));
    }
    let own = project_excludes(root)?;
    for pattern in &own {
        ensure!(
            !pattern.trim_start().starts_with('!'),
            "sync.exclude in borrow.toml cannot contain negated patterns: {pattern}"
        );
    }
    patterns.extend(own);
    validate_excludes(&patterns)?;
    Ok(patterns)
}

fn pattern_lines(file: &Path) -> Vec<String> {
    fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .collect()
}

/// The git global excludes file: `core.excludesFile` when git names one, otherwise
/// the default location under the XDG configuration directory.
fn global_excludes_file(root: &Path) -> Option<std::path::PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let configured = std::process::Command::new("git")
        .args(["config", "--get", "core.excludesFile"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|value| !value.is_empty());
    match configured {
        Some(value) if value.starts_with("~/") => Some(home.join(&value[2..])),
        Some(value) => Some(root.join(value)),
        None => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
            Some(base.join("git/ignore"))
        }
    }
}

#[derive(Default, Deserialize)]
struct Settings {
    #[serde(default)]
    sync: SyncSettings,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncSettings {
    #[serde(default)]
    exclude: Vec<String>,
}

/// Read `sync.exclude` from `borrow.toml`.
pub fn project_excludes(root: &Path) -> anyhow::Result<Vec<String>> {
    let file = root.join("borrow.toml");
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("Could not read {}", file.display())),
    };
    let settings: Settings =
        toml::from_str(&text).with_context(|| format!("Could not parse {}", file.display()))?;
    Ok(settings.sync.exclude)
}

pub fn validate_excludes(patterns: &[String]) -> anyhow::Result<()> {
    ensure!(patterns.len() <= 10_000, "Too many exclusion patterns");
    for pattern in patterns {
        ensure!(
            !pattern.chars().any(char::is_control) && pattern.len() <= 4096,
            "Invalid exclusion pattern: {pattern:?}"
        );
    }
    Ok(())
}

/// Eligibility rules shared by both machines: mandatory exclusions, `.gitignore` files
/// inside the project, and the extra patterns from `client_excludes`. Nothing is read
/// from outside the project, so each machine's own account setup cannot change results.
/// Extra patterns cannot override mandatory exclusions or `.gitignore` files.
pub struct Rules {
    extra: Gitignore,
}

impl Rules {
    pub fn new(root: &Path, patterns: &[String]) -> anyhow::Result<Rules> {
        validate_excludes(patterns)?;
        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            builder
                .add_line(None, pattern)
                .with_context(|| format!("Invalid exclusion pattern: {pattern}"))?;
        }
        Ok(Rules {
            extra: builder.build()?,
        })
    }

    /// Mandatory and extra exclusions for a single path, without `.gitignore` files.
    pub fn excluded(&self, name: &str, has_file: impl Fn(&str) -> bool) -> bool {
        if mandatory(name, has_file) {
            return true;
        }
        let path = Path::new(name);
        path.ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .enumerate()
            .any(|(depth, p)| self.extra.matched(p, depth > 0).is_ignore())
    }
}

/// Cached hashes keyed by path, reused while size, times, inode, and mode are unchanged.
#[derive(Default, Serialize, Deserialize)]
pub struct HashCache(HashMap<String, Stamp>);

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
    ino: u64,
    mode: u32,
    hash: String,
}

impl Stamp {
    fn matches(&self, meta: &fs::Metadata) -> bool {
        self.len == meta.len()
            && self.mtime == meta.mtime()
            && self.mtime_nsec == meta.mtime_nsec()
            && self.ctime == meta.ctime()
            && self.ctime_nsec == meta.ctime_nsec()
            && self.ino == meta.ino()
            && self.mode == meta.mode()
    }
}

/// Describe the file at a relative path, or `None` when nothing is there.
/// Directories and special files are errors, because callers compare file states.
pub fn entry(root: &Path, name: &str) -> anyhow::Result<Option<Entry>> {
    entry_cached(root, name, None)
}

fn entry_cached(
    root: &Path,
    name: &str,
    cache: Option<(&HashCache, &mut HashCache)>,
) -> anyhow::Result<Option<Entry>> {
    let path = storage::safe_path(root, name)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("Could not inspect {name}")),
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(&path)?;
        let target = target
            .to_str()
            .with_context(|| format!("Link target is not UTF 8: {name}"))?
            .to_string();
        return Ok(Some(Entry {
            hash: digest(target.as_bytes()),
            executable: false,
            link: Some(target),
        }));
    }
    ensure!(!meta.is_dir(), "A directory is in the way of {name}");
    ensure!(meta.is_file(), "Unsupported file type at {name}");
    let executable = meta.permissions().mode() & 0o111 != 0;
    if let Some((old, _)) = &cache
        && let Some(stamp) = old.0.get(name)
        && stamp.matches(&meta)
    {
        let hash = stamp.hash.clone();
        if let Some((_, new)) = cache {
            new.0.insert(name.to_string(), stamp.clone());
        }
        return Ok(Some(Entry {
            hash,
            executable,
            link: None,
        }));
    }
    let hash = hash_file(&path).with_context(|| format!("Could not read {name}"))?;
    if let Some((_, new)) = cache
        && settled(&meta)
    {
        new.0.insert(
            name.to_string(),
            Stamp {
                len: meta.len(),
                mtime: meta.mtime(),
                mtime_nsec: meta.mtime_nsec(),
                ctime: meta.ctime(),
                ctime_nsec: meta.ctime_nsec(),
                ino: meta.ino(),
                mode: meta.mode(),
                hash: hash.clone(),
            },
        );
    }
    Ok(Some(Entry {
        hash,
        executable,
        link: None,
    }))
}

/// A file modified within the last two seconds could change again without a visible
/// timestamp difference on coarse file systems, so its hash is not cached yet.
fn settled(meta: &fs::Metadata) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    meta.mtime() < now - 2
}

pub fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hasher.update(&buffer[..size]);
    }
    Ok(hex(&hasher.finalize()))
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Walk the project and fingerprint every eligible file.
///
/// Ignored and generated folders are pruned before descending, so nothing inside
/// them is read, and a nested negation cannot bring back a file in an ignored folder.
///
/// A file that was synchronized before but is now filtered out keeps its baseline
/// entry while it still exists. Changing ignore rules therefore never deletes or
/// transfers anything by itself; only real edits and real deletions do.
pub fn scan(
    root: &Path,
    rules: &Rules,
    baseline: &Manifest,
    cache_file: Option<&Path>,
) -> anyhow::Result<Manifest> {
    let old_cache: HashCache = match cache_file {
        Some(file) => storage::read_json(file).unwrap_or_default(),
        None => HashCache::default(),
    };
    let mut new_cache = HashCache::default();
    let mut manifest = Manifest::new();

    let base = root.to_path_buf();
    let extra = rules.extra.clone();
    let mut walker = ignore::WalkBuilder::new(root);
    walker
        .standard_filters(false)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(true)
        .git_exclude(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |item| {
            let Ok(rel) = item.path().strip_prefix(&base) else {
                return false;
            };
            if rel.as_os_str().is_empty() {
                return true;
            }
            let Some(rel) = rel.to_str() else {
                return true;
            };
            let is_dir = item.file_type().is_some_and(|t| t.is_dir());
            let name = rel.rsplit('/').next().unwrap_or(rel);
            let parent = base.join(rel).parent().map(Path::to_path_buf);
            let generated = mandatory(name, |cargo| {
                parent.as_ref().is_some_and(|dir| dir.join(cargo).is_file())
            });
            !generated && !extra.matched(rel, is_dir).is_ignore()
        });

    for item in walker.build() {
        let item = item.context("Could not read the project tree")?;
        let Some(kind) = item.file_type() else {
            continue;
        };
        if kind.is_dir() || !(kind.is_file() || kind.is_symlink()) {
            continue;
        }
        let rel = item.path().strip_prefix(root)?;
        let Some(name) = rel.to_str() else {
            bail!(
                "Source path is not UTF 8: {}. Add it to .gitignore to skip it",
                rel.display()
            );
        };
        storage::relative(name).with_context(|| {
            format!("Unsupported source path {name:?}. Add it to .gitignore to skip it")
        })?;
        if let Some(value) = entry_cached(root, name, Some((&old_cache, &mut new_cache)))? {
            manifest.insert(name.to_string(), value);
        }
    }

    for (name, value) in baseline {
        if manifest.contains_key(name) || mandatory(name, |f| manifest.contains_key(f)) {
            continue;
        }
        if matches!(entry(root, name), Ok(Some(_))) {
            manifest.insert(name.clone(), value.clone());
        }
    }

    check_links(&manifest, rules)?;

    if let Some(file) = cache_file {
        let _ = storage::write_json(file, &new_cache);
    }
    Ok(manifest)
}

/// Refuse links that leave the project or point at excluded paths. Resolution follows
/// links recorded in the manifest itself, so the check is the same on both machines
/// and does not depend on which targets happen to exist yet.
pub fn check_links(manifest: &Manifest, rules: &Rules) -> anyhow::Result<()> {
    for (name, value) in manifest {
        if value.link.is_some() {
            resolve(manifest, name, rules).with_context(|| {
                format!("Unsafe source link {name}. Remove it or add it to .gitignore to skip it")
            })?;
        }
    }
    Ok(())
}

fn resolve(manifest: &Manifest, name: &str, rules: &Rules) -> anyhow::Result<()> {
    let mut parts: Vec<String> = name.split('/').map(str::to_string).collect();
    parts.pop();
    let target = manifest[name].link.as_deref().unwrap_or_default();
    let resolved = follow(manifest, parts, target, 0)?;
    let path = resolved.join("/");
    if !path.is_empty() {
        ensure!(
            !rules.excluded(&path, |f| manifest.contains_key(f)),
            "It points at an excluded path"
        );
    }
    Ok(())
}

fn follow(
    manifest: &Manifest,
    mut position: Vec<String>,
    target: &str,
    depth: usize,
) -> anyhow::Result<Vec<String>> {
    ensure!(depth < 40, "Too many nested links");
    ensure!(
        !target.starts_with('/'),
        "Absolute link targets leave the project"
    );
    ensure!(
        !target.chars().any(char::is_control),
        "Link target has control characters"
    );
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(position.pop().is_some(), "It points outside the project");
            }
            normal => {
                position.push(normal.to_string());
                let joined = position.join("/");
                if let Some(inner) = manifest.get(&joined).and_then(|e| e.link.as_deref()) {
                    position.pop();
                    position = follow(manifest, position, inner, depth + 1)?;
                }
            }
        }
    }
    Ok(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::TempDir;
    use std::os::unix::fs::symlink;

    fn rules(root: &Path, patterns: &[&str]) -> Rules {
        let patterns: Vec<String> = patterns.iter().map(|p| p.to_string()).collect();
        Rules::new(root, &patterns).unwrap()
    }

    fn names(manifest: &Manifest) -> Vec<&str> {
        manifest.keys().map(String::as_str).collect()
    }

    #[test]
    fn environment_names_include_templates_at_any_depth() {
        for name in [".env", ".env.local", ".env.example", "prod.env", ".envrc"] {
            assert!(is_environment_name(name), "{name}");
            assert!(mandatory(&format!("api/deep/{name}"), |_| false), "{name}");
        }
        for name in ["environment.rs", "envoy.yaml", "env", ".envy"] {
            assert!(!is_environment_name(name), "{name}");
        }
    }

    #[test]
    fn target_is_generated_only_beside_cargo_toml() {
        assert!(mandatory("target/debug/app", |f| f == "Cargo.toml"));
        assert!(mandatory("crates/a/target/x", |f| f == "crates/a/Cargo.toml"));
        assert!(!mandatory("src/target/mod.rs", |f| f == "Cargo.toml"));
        assert!(mandatory("web/node_modules/x/index.js", |_| false));
        assert!(mandatory(".git/config", |_| false));
        assert!(mandatory("a/.borrow-partial-123", |_| false));
    }

    #[test]
    fn scan_skips_ignored_generated_and_environment_files() {
        let dir = TempDir::new("scan");
        dir.write(".gitignore", "logs/\n*.tmp\n");
        dir.write("Cargo.toml", "");
        dir.write("src/main.rs", "fn main() {}");
        dir.write("src/target/mod.rs", "");
        dir.write("target/debug/app", "binary");
        dir.write("logs/today.log", "");
        dir.write("notes.tmp", "");
        dir.write(".env", "SECRET=1");
        dir.write("api/.env.example", "SECRET=");
        dir.write("node_modules/pkg/index.js", "");
        dir.write(".git/config", "");
        let manifest = scan(dir.path(), &rules(dir.path(), &[]), &Manifest::new(), None).unwrap();
        assert_eq!(
            names(&manifest),
            [
                ".gitignore",
                "Cargo.toml",
                "src/main.rs",
                "src/target/mod.rs"
            ]
        );
    }

    #[test]
    fn a_nested_negation_cannot_reinclude_a_file_in_an_ignored_folder() {
        let dir = TempDir::new("negation");
        dir.write(".gitignore", "build/\n");
        dir.write("build/.gitignore", "!keep.txt\n");
        dir.write("build/keep.txt", "");
        dir.write("docs/.gitignore", "*.md\n!README.md\n");
        dir.write("docs/README.md", "");
        dir.write("docs/other.md", "");
        let manifest = scan(dir.path(), &rules(dir.path(), &[]), &Manifest::new(), None).unwrap();
        assert_eq!(
            names(&manifest),
            [".gitignore", "docs/.gitignore", "docs/README.md"]
        );
    }

    #[test]
    fn repository_excludes_and_project_patterns_apply() {
        let dir = TempDir::new("excludes");
        dir.write(".git/info/exclude", "private.txt\n");
        dir.write("private.txt", "");
        dir.write("data/big.bin", "");
        dir.write("data/small.txt", "");
        dir.write("keep.txt", "");
        dir.write("borrow.toml", "[sync]\nexclude = [\"data/*.bin\"]\n");
        let patterns = client_excludes(dir.path()).unwrap();
        assert!(patterns.contains(&"private.txt".to_string()));
        let rules = Rules::new(dir.path(), &patterns).unwrap();
        let manifest = scan(dir.path(), &rules, &Manifest::new(), None).unwrap();
        assert_eq!(
            names(&manifest),
            ["borrow.toml", "data/small.txt", "keep.txt"]
        );
        assert!(rules.excluded("data/big.bin", |_| false));
        assert!(!rules.excluded("data/small.txt", |_| false));
    }

    #[test]
    fn negated_project_patterns_are_refused() {
        let dir = TempDir::new("negated");
        dir.write("borrow.toml", "[sync]\nexclude = [\"!.env\"]\n");
        assert!(client_excludes(dir.path()).is_err());
        dir.write("borrow.toml", "[sync]\nexclude = [\"*.bin\"]\n");
        assert!(
            client_excludes(dir.path())
                .unwrap()
                .ends_with(&["*.bin".to_string()])
        );
        let rules = Rules::new(dir.path(), &["*.log".into(), "!keep.log".into()]).unwrap();
        assert!(rules.excluded("a.log", |_| false));
        assert!(!rules.excluded("keep.log", |_| false));
        assert!(rules.excluded(".env", |_| false));
    }

    #[test]
    fn a_newly_ignored_file_keeps_its_baseline_entry() {
        let dir = TempDir::new("freeze");
        dir.write("config.json", "old");
        let first = scan(dir.path(), &rules(dir.path(), &[]), &Manifest::new(), None).unwrap();
        dir.write(".gitignore", "config.json\n");
        dir.write("config.json", "edited while ignored");
        let second = scan(dir.path(), &rules(dir.path(), &[]), &first, None).unwrap();
        assert_eq!(second.get("config.json"), first.get("config.json"));
        std::fs::remove_file(dir.path().join("config.json")).unwrap();
        let third = scan(dir.path(), &rules(dir.path(), &[]), &first, None).unwrap();
        assert!(!third.contains_key("config.json"));
    }

    #[test]
    fn safe_relative_links_are_kept_and_escaping_links_are_refused() {
        let dir = TempDir::new("links");
        dir.write("src/lib.rs", "");
        symlink("src/lib.rs", dir.path().join("alias.rs")).unwrap();
        symlink("src", dir.path().join("code")).unwrap();
        let manifest = scan(dir.path(), &rules(dir.path(), &[]), &Manifest::new(), None).unwrap();
        assert_eq!(manifest["alias.rs"].link.as_deref(), Some("src/lib.rs"));
        assert!(manifest.contains_key("code"));

        symlink("../outside", dir.path().join("escape")).unwrap();
        assert!(scan(dir.path(), &rules(dir.path(), &[]), &Manifest::new(), None).is_err());
    }

    #[test]
    fn link_resolution_follows_other_links_before_checking() {
        let r = rules(Path::new("/project"), &[]);
        let link = |target: &str| Entry {
            hash: String::new(),
            executable: false,
            link: Some(target.to_string()),
        };
        let mut manifest = Manifest::from([("here".to_string(), link("."))]);
        manifest.insert("sneaky".to_string(), link("here/../secret"));
        assert!(check_links(&manifest, &r).is_err());

        let mut manifest = Manifest::from([("deep".to_string(), link("a/b"))]);
        manifest.insert("up".to_string(), link("deep/../c"));
        assert!(check_links(&manifest, &r).is_ok());

        let manifest = Manifest::from([("env".to_string(), link("config/.env"))]);
        assert!(check_links(&manifest, &r).is_err());

        let mut manifest = Manifest::from([("loop".to_string(), link("loop2"))]);
        manifest.insert("loop2".to_string(), link("loop"));
        assert!(check_links(&manifest, &r).is_err());

        let manifest = Manifest::from([("abs".to_string(), link("/etc/passwd"))]);
        assert!(check_links(&manifest, &r).is_err());
    }

    #[test]
    fn cached_hashes_are_reused_and_refreshed_on_change() {
        let dir = TempDir::new("cache");
        let state = TempDir::new("cache-state");
        let file = dir.write("a.txt", "one");
        let past = SystemTime::now() - std::time::Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let cache = state.path().join("hashes.json");
        let r = rules(dir.path(), &[]);
        let first = scan(dir.path(), &r, &Manifest::new(), Some(&cache)).unwrap();
        assert!(std::fs::read_to_string(&cache).unwrap().contains("a.txt"));
        let second = scan(dir.path(), &r, &Manifest::new(), Some(&cache)).unwrap();
        assert_eq!(first, second);
        dir.write("a.txt", "two");
        let third = scan(dir.path(), &r, &Manifest::new(), Some(&cache)).unwrap();
        assert_ne!(first["a.txt"].hash, third["a.txt"].hash);
    }

    #[test]
    fn executable_bits_are_recorded() {
        let dir = TempDir::new("exec");
        let script = dir.write("run.sh", "#!/bin/sh");
        std::fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(entry(dir.path(), "run.sh").unwrap().unwrap().executable);
        assert!(entry(dir.path(), "missing").unwrap().is_none());
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        assert!(entry(dir.path(), "folder").is_err());
    }
}
