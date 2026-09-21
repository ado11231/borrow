//! Private Borrow storage: directories, identifiers, atomic writes, and locks.

use anyhow::{Context, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Borrow's platform data directory. The Client and Agent each use their own subfolder,
/// so one machine can safely play both roles.
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "borrow")
        .context("Could not determine the home directory for Borrow storage")?;
    Ok(dirs.data_local_dir().to_path_buf())
}

/// Create a directory readable only by this account, refusing a symlink in its place.
pub fn private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).with_context(|| format!("Could not create {}", path.display()))?;
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Storage path must be a real directory: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Names the temporary file an atomic write renames into place. Source scanning skips
/// anything starting with it, so a write in flight is never picked up as project content.
pub const PARTIAL_PREFIX: &str = ".borrow-partial-";

/// A random 128 bit identifier written as 32 lowercase hex characters.
pub fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// The leading part of an identifier, which is what job listings show and `borrow stop`
/// accepts. Both sides shorten the same way so a copied prefix always matches.
pub fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

pub fn check_id(value: &str) -> anyhow::Result<()> {
    ensure!(
        value.len() == 32
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Invalid Borrow identifier"
    );
    Ok(())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Accept only plain relative paths such as `src/main.rs`. Rejects absolute paths,
/// `.` and `..` segments, and characters that would break rsync file lists.
pub fn relative(value: &str) -> anyhow::Result<&Path> {
    let path = Path::new(value);
    ensure!(
        !value.is_empty()
            && !value.ends_with('/')
            && !value.contains("//")
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Expected a relative project path: {value:?}"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "Unsupported control character in path: {value:?}"
    );
    Ok(path)
}

/// Join a relative path under a root after confirming no existing parent is a symlink,
/// so writes can never be redirected outside the root.
pub fn safe_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let rel = relative(name)?;
    let meta = fs::symlink_metadata(root)
        .with_context(|| format!("Missing directory {}", root.display()))?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Root must be a real directory: {}",
        root.display()
    );
    let mut current = root.to_path_buf();
    let parts: Vec<_> = rel.components().collect();
    for part in &parts[..parts.len() - 1] {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Unsafe parent path for {name}"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(root.join(rel))
}

/// Create missing parents of a safe path, checking each level as it is created.
pub fn create_parents(root: &Path, name: &str) -> anyhow::Result<()> {
    let rel = relative(name)?;
    let mut current = root.to_path_buf();
    let parts: Vec<_> = rel.components().collect();
    for part in &parts[..parts.len() - 1] {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Unsafe parent path for {name}"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    write_bytes(path, &serde_json::to_vec(value)?)
}

/// Replace a file atomically with owner only permissions. The parent must already exist.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("Missing parent directory")?;
    let temp = parent.join(format!("{PARTIAL_PREFIX}{}", new_id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.with_context(|| format!("Could not write {}", path.display()))
}

/// Read JSON, treating a missing file as the default value.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> anyhow::Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("Could not parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("Could not read {}", path.display())),
    }
}

/// Open a lock file without disturbing whatever is already in it.
fn lock_file(path: &Path) -> anyhow::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("Could not open {}", path.display()))
}

/// Take an exclusive advisory lock without waiting. The lock lasts as long as the
/// returned file stays open, and the operating system drops it if the process dies.
pub fn try_lock(path: &Path) -> anyhow::Result<Option<File>> {
    let file = lock_file(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// Take an exclusive lock, waiting for other holders. Used only for short updates.
pub fn lock(path: &Path) -> anyhow::Result<File> {
    let file = lock_file(path)?;
    file.lock()?;
    Ok(file)
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    /// A temporary directory removed when the test ends.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(label: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!("borrow-{label}-{}", super::new_id()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path.canonicalize().unwrap())
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TempDir;
    use super::*;

    #[test]
    fn only_plain_relative_paths_are_accepted() {
        for good in ["a", "src/main.rs", ".github/workflows/ci.yml"] {
            assert!(relative(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../b",
            "./a",
            "a/",
            "a//b",
            "a\nb",
            "a\0b",
        ] {
            assert!(relative(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn identifiers_are_lowercase_hex_of_fixed_length() {
        assert!(check_id(&new_id()).is_ok());
        assert!(check_id("ABCDEF0123456789abcdef0123456789").is_err());
        assert!(check_id("../../../../etc").is_err());
    }

    #[test]
    fn a_symlinked_parent_is_refused() {
        let dir = TempDir::new("safe");
        let outside = TempDir::new("outside");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(safe_path(dir.path(), "link/file").is_err());
        assert!(create_parents(dir.path(), "link/file").is_err());
        assert!(safe_path(dir.path(), "missing/deeper/file").is_ok());
        create_parents(dir.path(), "real/deeper/file").unwrap();
        assert!(dir.path().join("real/deeper").is_dir());
    }

    #[test]
    fn a_held_lock_is_reported_as_busy() {
        let dir = TempDir::new("lock");
        let file = dir.path().join("work.lock");
        let held = try_lock(&file).unwrap();
        assert!(held.is_some());
        assert!(try_lock(&file).unwrap().is_none());
        drop(held);
        let released = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            try_lock(&file).unwrap().is_some()
        });
        assert!(released);
    }

    #[test]
    fn atomic_writes_are_private() {
        let dir = TempDir::new("write");
        let file = dir.path().join("state.json");
        write_json(&file, &vec!["a"]).unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let back: Vec<String> = read_json(&file).unwrap();
        assert_eq!(back, ["a"]);
        let missing: Vec<String> = read_json(&dir.path().join("none.json")).unwrap();
        assert!(missing.is_empty());
    }
}
