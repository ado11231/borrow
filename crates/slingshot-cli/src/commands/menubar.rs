//! `slingshot menubar`: open the Slingshot menu bar app and tell it where this program is.
//! The app starts `slingshot internal-watch` for its numbers, and an app opened from
//! Finder or at login does not get the shell's PATH, so it cannot find the program alone.

/// The app's bundle identifier, which is also where its settings live.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "dev.slingshot.menubar";

/// A tip for the end of `link`, on the only platform with a menu bar app.
pub fn tip(name: &str) -> Option<String> {
    cfg!(target_os = "macos").then(|| format!("Keep {name} in your menu bar: slingshot menubar"))
}

#[cfg(not(target_os = "macos"))]
pub async fn menubar(_agent: Option<String>) -> anyhow::Result<i32> {
    anyhow::bail!(
        "The menu bar app is macOS only for now. slingshot health --watch shows the same numbers in a terminal"
    )
}

#[cfg(target_os = "macos")]
pub async fn menubar(agent: Option<String>) -> anyhow::Result<i32> {
    use anyhow::Context;
    use slingshot_core::config::Config;
    use slingshot_core::presentation::{self, home_path};

    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let app = find_app().context(
        "Slingshot.app is not installed. Build and install it from the Slingshot source folder with mac/menubar/build.sh",
    )?;
    let program = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("Could not find this program's own path")?;

    defaults(&[
        "write",
        BUNDLE_ID,
        "slingshotPath",
        &program.to_string_lossy(),
    ])?;
    match &agent {
        Some(name) => defaults(&["write", BUNDLE_ID, "agent", name])?,
        None => {
            let _ = defaults(&["delete", BUNDLE_ID, "agent"]);
        }
    }
    let opened = std::process::Command::new("open")
        .arg(&app)
        .status()
        .context("Could not start open")?;
    anyhow::ensure!(
        opened.success(),
        "Could not open {}. Try opening it from Finder",
        home_path(&app)
    );

    presentation::success(format!("{} is in your menu bar", target.name));
    presentation::detail("App", home_path(&app));
    presentation::detail("Starts", "at login, turn off in the app's menu");
    Ok(0)
}

#[cfg(target_os = "macos")]
fn find_app() -> Option<std::path::PathBuf> {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().join("Applications"));
    home.into_iter()
        .chain(Some(std::path::PathBuf::from("/Applications")))
        .map(|folder| folder.join("Slingshot.app"))
        .find(|app| app.is_dir())
}

#[cfg(target_os = "macos")]
fn defaults(args: &[&str]) -> anyhow::Result<()> {
    let status = std::process::Command::new("defaults")
        .args(args)
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| anyhow::anyhow!("Could not run defaults: {error}"))?;
    anyhow::ensure!(
        status.success(),
        "Could not save the menu bar app's settings"
    );
    Ok(())
}
