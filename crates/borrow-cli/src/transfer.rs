//! Copying eligible source between the Client and an Agent.
//!
//! Both sides list their files, the three way plan decides what moves, rsync copies
//! only the changed regular files into staging over SSH, and the receiving side checks
//! every staged file before applying anything.

use crate::client::{Control, unexpected};
use crate::project::{self, Local};
use anyhow::{Context, bail, ensure};
use borrow_core::config::Agent;
use borrow_core::control::{ProjectInfo, ProjectRef, Request, Response, Snapshot};
use borrow_core::presentation::{self, Style, Tone};
use borrow_core::source::{self, Manifest, Rules};
use borrow_core::storage;
use borrow_core::sync::{self, Plan, StateDir};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// Ping interval while rsync runs, so the Agent keeps the lease for a live Client.
const HEARTBEAT: Duration = Duration::from_secs(20);

pub struct ProjectSession {
    pub control: Control,
    pub project: ProjectInfo,
    pub id: String,
}

/// Connect to the Agent and make sure the project has storage there.
pub async fn open(agent: &Agent, local: &Local) -> anyhow::Result<ProjectSession> {
    let id = project::identify(&project::client_root()?, &local.root, &agent.name)?;
    let mut control = Control::connect(agent).await?;
    let reference = ProjectRef {
        id: id.clone(),
        name: local.name.clone(),
        client: borrow_core::keys::client_name(),
    };
    let Response::Project(project) = control.call(Request::Open(reference)).await? else {
        return Err(unexpected());
    };
    Ok(ProjectSession {
        control,
        project,
        id,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Push,
    Pull,
}

#[derive(Default)]
pub struct SyncResult {
    pub changed: usize,
    pub kept: usize,
}

/// Finish or undo an interrupted pull on this machine before its source is read again.
async fn recover_local(local: &Local, id: &str) -> anyhow::Result<()> {
    let state = StateDir::new(state_dir(id)?);
    let root = local.root.clone();
    tokio::task::spawn_blocking(move || sync::recover(&root, &state)).await?
}

/// Preview a sync without taking a lease or changing anything.
pub async fn preview(
    opened: &mut ProjectSession,
    agent: &Agent,
    local: &Local,
    direction: Direction,
) -> anyhow::Result<Plan> {
    recover_local(local, &opened.id).await?;
    let excludes = source::client_excludes(&local.root)?;
    let snapshot = match opened
        .control
        .call(Request::Inspect {
            project: opened.id.clone(),
            excludes: excludes.clone(),
        })
        .await?
    {
        Response::Snapshot(snapshot) => snapshot,
        _ => return Err(unexpected()),
    };
    let (local_manifest, _) = scan_local(local, &opened.id, &excludes, &snapshot.baseline).await?;
    let plan = match direction {
        Direction::Push => sync::plan(&snapshot.baseline, &local_manifest, &snapshot.manifest),
        Direction::Pull => sync::plan(&snapshot.baseline, &snapshot.manifest, &local_manifest),
    };
    let (sender, receiver) = match direction {
        Direction::Push => (&local_manifest, &snapshot.manifest),
        Direction::Pull => (&snapshot.manifest, &local_manifest),
    };
    print!(
        "{}",
        preview_text(
            &plan,
            sender,
            receiver,
            &agent.name,
            direction,
            Style::stdout()
        )
    );
    Ok(plan)
}

/// Copy Client edits to the Agent. Nothing is printed unless files move.
pub async fn push(
    opened: &mut ProjectSession,
    agent: &Agent,
    local: &Local,
) -> anyhow::Result<SyncResult> {
    recover_local(local, &opened.id).await?;
    let excludes = source::client_excludes(&local.root)?;
    let snapshot = begin(opened, &excludes, false).await?;
    let token = snapshot
        .token
        .clone()
        .context("The Agent did not open a sync")?;
    let result = async {
        let (local_manifest, _) =
            scan_local(local, &opened.id, &excludes, &snapshot.baseline).await?;
        let plan = sync::plan(&snapshot.baseline, &local_manifest, &snapshot.manifest);
        refuse_conflicts(&plan, &agent.name)?;
        let files = regular_files(&plan, &local_manifest);
        if !files.is_empty() {
            presentation::progress(format!(
                "Copying {} to {}",
                presentation::plural(files.len(), "file"),
                agent.name
            ));
            let transfer = rsync(
                agent,
                &local.root,
                &snapshot.transfer_path,
                &files,
                Direction::Push,
            );
            with_heartbeat(&mut opened.control, transfer).await?;
        }
        let Response::Synced { changed } = opened
            .control
            .call(Request::Finish {
                token: token.clone(),
                manifest: local_manifest,
            })
            .await?
        else {
            return Err(unexpected());
        };
        Ok(SyncResult {
            changed,
            kept: plan.kept.len(),
        })
    }
    .await;
    if result.is_err() {
        let _ = opened.control.call(Request::Release { token }).await;
    }
    let outcome = result?;
    if outcome.changed > 0 {
        presentation::success(format!(
            "Synced {} to {}",
            presentation::plural(outcome.changed, "change"),
            agent.name
        ));
    }
    Ok(outcome)
}

/// Copy Agent edits back to the Client.
pub async fn pull(
    opened: &mut ProjectSession,
    agent: &Agent,
    local: &Local,
) -> anyhow::Result<SyncResult> {
    recover_local(local, &opened.id).await?;
    let excludes = source::client_excludes(&local.root)?;
    let state_dir = state_dir(&opened.id)?;
    let state = StateDir::new(&state_dir);

    let snapshot = begin(opened, &excludes, true).await?;
    let token = snapshot
        .token
        .clone()
        .context("The Agent did not open a sync")?;
    let stage = state_dir.join("staging").join(&token);
    let result = async {
        let (local_manifest, rules) =
            scan_local(local, &opened.id, &excludes, &snapshot.baseline).await?;
        let plan = sync::plan(&snapshot.baseline, &snapshot.manifest, &local_manifest);
        refuse_conflicts(&plan, &agent.name)?;
        for name in &plan.changes {
            ensure!(
                !rules.excluded(name, |f| snapshot.manifest.contains_key(f)),
                "{} sent an excluded path: {name}",
                agent.name
            );
        }
        let applied_manifest = sync::merge(&local_manifest, &plan.changes, &snapshot.manifest);
        source::check_links(&applied_manifest, &rules)?;

        let files = regular_files(&plan, &snapshot.manifest);
        storage::private_dir(&stage)?;
        if !files.is_empty() {
            presentation::progress(format!(
                "Copying {} from {}",
                presentation::plural(files.len(), "file"),
                agent.name
            ));
            let transfer = rsync(
                agent,
                &stage,
                &snapshot.transfer_path,
                &files,
                Direction::Pull,
            );
            with_heartbeat(&mut opened.control, transfer).await?;
        }
        if !plan.changes.is_empty() {
            let (root, stage, plan, receiver, sender, token) = (
                local.root.clone(),
                stage.clone(),
                plan.clone(),
                local_manifest.clone(),
                snapshot.manifest.clone(),
                token.clone(),
            );
            let state = StateDir::new(&state.dir);
            tokio::task::spawn_blocking(move || {
                sync::apply(&root, &stage, &state, &plan, &receiver, &sender, &token)
            })
            .await??;
        }
        opened
            .control
            .call(Request::Finish {
                token: token.clone(),
                manifest: applied_manifest,
            })
            .await?;
        Ok(SyncResult {
            changed: plan.changes.len(),
            kept: plan.kept.len(),
        })
    }
    .await;
    let _ = std::fs::remove_dir_all(state_dir.join("staging"));
    if result.is_err() {
        let _ = opened.control.call(Request::Release { token }).await;
    }
    result
}

async fn begin(
    opened: &mut ProjectSession,
    excludes: &[String],
    pull: bool,
) -> anyhow::Result<Snapshot> {
    match opened
        .control
        .call(Request::Begin {
            project: opened.id.clone(),
            excludes: excludes.to_vec(),
            pull,
        })
        .await?
    {
        Response::Snapshot(snapshot) => Ok(snapshot),
        _ => Err(unexpected()),
    }
}

fn state_dir(id: &str) -> anyhow::Result<PathBuf> {
    storage::check_id(id)?;
    let dir = project::client_root()?.join("projects").join(id);
    storage::private_dir(&dir)?;
    Ok(dir)
}

async fn scan_local(
    local: &Local,
    id: &str,
    excludes: &[String],
    baseline: &Manifest,
) -> anyhow::Result<(Manifest, Rules)> {
    let cache = state_dir(id)?.join("hashes.json");
    let (root, excludes, baseline) = (local.root.clone(), excludes.to_vec(), baseline.clone());
    tokio::task::spawn_blocking(move || {
        let rules = Rules::new(&root, &excludes)?;
        let manifest = source::scan(&root, &rules, &baseline, Some(&cache))?;
        Ok((manifest, rules))
    })
    .await?
}

fn refuse_conflicts(plan: &Plan, agent: &str) -> anyhow::Result<()> {
    if plan.conflicts.is_empty() {
        return Ok(());
    }
    let listed: Vec<String> = plan
        .conflicts
        .iter()
        .map(|name| format!("  {name}"))
        .collect();
    bail!(
        "These paths changed differently on this machine and {agent}:\n{}\nNothing was changed. Make each path match on both machines, or undo one side's edit, then sync again. Compare with borrow sync --check and borrow sync --pull --check",
        listed.join("\n")
    )
}

fn regular_files(plan: &Plan, sender: &Manifest) -> Vec<String> {
    plan.changes
        .iter()
        .filter(|name| sender.get(*name).is_some_and(|entry| entry.link.is_none()))
        .cloned()
        .collect()
}

/// Run rsync with an explicit file list. The remote side changes into the transfer
/// folder first, so no Agent path passes through rsync's own remote argument handling.
async fn rsync(
    agent: &Agent,
    local: &Path,
    remote: &str,
    files: &[String],
    direction: Direction,
) -> anyhow::Result<()> {
    ensure!(
        borrow_core::telemetry::is_installed("rsync"),
        "rsync is not installed on this machine. Fix: {}",
        borrow_core::preflight::install_hint("rsync")
    );
    let exe = std::env::current_exe()?;
    let exe = exe.to_str().context("Borrow's own path is not UTF 8")?;
    ensure!(
        !exe.contains('\''),
        "Borrow's own path contains a quote, which rsync cannot use: {exe}"
    );
    let list = tempfile(&files.join("\n"))?;
    let local_path = format!("{}/", local.display());
    let (from, to) = match direction {
        Direction::Push => (local_path, "borrow:.".to_string()),
        Direction::Pull => ("borrow:./".to_string(), local_path),
    };
    let rsync_path = format!("cd {} && rsync", escape_remote(remote)?);
    let mut child = tokio::process::Command::new("rsync")
        .arg("-e")
        .arg(format!("'{exe}' internal-rsh"))
        .arg(format!("--rsync-path={rsync_path}"))
        .arg(format!("--files-from={list}"))
        .arg(from)
        .arg(to)
        .env("BORROW_RSH_AGENT", &agent.name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Could not start rsync")?;
    let stderr = child.stderr.take().context("Missing rsync output")?;
    let mut errors = Vec::new();
    let mut limited = stderr.take(64 * 1024);
    let (status, _) = tokio::join!(child.wait(), limited.read_to_end(&mut errors));
    let _ = std::fs::remove_file(&list);
    let status = status?;
    if !status.success() {
        bail!(
            "Copying files with rsync failed. Nothing was applied\n{}",
            String::from_utf8_lossy(&errors).trim()
        );
    }
    Ok(())
}

/// Escape each unusual character with a backslash. Some rsync versions split the remote
/// command on spaces and drop quotes before SSH joins it again, but a backslash before
/// every space survives both that and versions that pass the command unchanged.
fn escape_remote(path: &str) -> anyhow::Result<String> {
    ensure!(
        !path.is_empty()
            && !path
                .chars()
                .any(|c| c == '\'' || c == '"' || c.is_control()),
        "The Agent storage path contains characters rsync cannot use: {path:?}"
    );
    Ok(path
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() || "/._-".contains(c) {
            true => c.to_string(),
            false => format!("\\{c}"),
        })
        .collect())
}

fn tempfile(body: &str) -> anyhow::Result<String> {
    let dir = project::client_root()?.join("lists");
    storage::private_dir(&dir)?;
    let file = dir.join(storage::new_id());
    storage::write_bytes(&file, format!("{body}\n").as_bytes())?;
    file.to_str()
        .map(str::to_string)
        .context("Borrow storage path is not UTF 8")
}

async fn with_heartbeat(
    control: &mut Control,
    work: impl std::future::Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    tokio::pin!(work);
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = ticker.tick() => {
                control.call(Request::Ping).await?;
            }
        }
    }
}

/// A readable preview of a plan. Words, not symbols, say what would happen.
pub fn preview_text(
    plan: &Plan,
    sender: &Manifest,
    receiver: &Manifest,
    agent: &str,
    direction: Direction,
    style: Style,
) -> String {
    let (from, to) = match direction {
        Direction::Push => ("this machine".to_string(), agent.to_string()),
        Direction::Pull => (agent.to_string(), "this machine".to_string()),
    };
    let mut output = format!(
        "{}\n",
        style.heading(format!("Sync preview from {from} to {to}"))
    );
    if plan.changes.is_empty() && plan.conflicts.is_empty() && plan.kept.is_empty() {
        output.push_str(&format!("  {to} already matches {from}\n"));
        return output;
    }
    for name in &plan.changes {
        let action = match (sender.contains_key(name), receiver.contains_key(name)) {
            (true, false) => "Add",
            (false, _) => "Delete",
            (true, true) => "Update",
        };
        output.push_str(&format!("  {:<9} {name}\n", action));
    }
    for name in &plan.conflicts {
        output.push_str(&format!(
            "  {} {name}\n",
            style.paint(format!("{:<9}", "Conflict"), Tone::Error)
        ));
    }
    for name in &plan.kept {
        output.push_str(&format!("  {:<9} {name} (changed only on {to})\n", "Keep"));
    }
    output.push_str(&format!(
        "\n  {} would be applied. Nothing was changed\n",
        presentation::plural(plan.changes.len(), "change")
    ));
    if !plan.conflicts.is_empty() {
        output.push_str(&format!(
            "  {} must be resolved first\n",
            presentation::plural(plan.conflicts.len(), "conflict")
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrow_core::source::Entry;

    fn file(hash: &str) -> Entry {
        Entry {
            hash: hash.into(),
            executable: false,
            link: None,
        }
    }

    #[test]
    fn previews_name_every_action_in_words() {
        let base = Manifest::from([
            ("old".into(), file("a")),
            ("same".into(), file("a")),
            ("both".into(), file("a")),
        ]);
        let sender = Manifest::from([
            ("new".into(), file("n")),
            ("same".into(), file("b")),
            ("both".into(), file("b")),
        ]);
        let receiver = Manifest::from([
            ("old".into(), file("a")),
            ("same".into(), file("a")),
            ("both".into(), file("c")),
            ("mine".into(), file("m")),
        ]);
        let plan = sync::plan(&base, &sender, &receiver);
        let text = preview_text(
            &plan,
            &sender,
            &receiver,
            "archbox",
            Direction::Push,
            Style::new(false),
        );
        assert!(text.contains("Sync preview from this machine to archbox"));
        assert!(text.contains("Add       new"));
        assert!(text.contains("Delete    old"));
        assert!(text.contains("Update    same"));
        assert!(text.contains("Conflict  both"));
        assert!(text.contains("Keep      mine (changed only on archbox)"));
        assert!(text.contains("3 changes would be applied. Nothing was changed"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn links_are_not_sent_through_rsync() {
        let link = Entry {
            link: Some("a".into()),
            ..file("l")
        };
        let sender = Manifest::from([("a".into(), file("x")), ("b".into(), link)]);
        let plan = sync::plan(&Manifest::new(), &sender, &Manifest::new());
        assert_eq!(regular_files(&plan, &sender), ["a"]);
    }

    #[test]
    fn remote_paths_survive_rsync_splitting() {
        assert_eq!(
            escape_remote("/Users/me/Library/Application Support/borrow").unwrap(),
            "/Users/me/Library/Application\\ Support/borrow"
        );
        assert_eq!(escape_remote("/a/$(x)&b").unwrap(), "/a/\\$\\(x\\)\\&b");
        assert!(escape_remote("/it's").is_err());
        assert!(escape_remote("/a\nb").is_err());
    }

    #[test]
    fn conflicts_are_listed_with_a_way_forward() {
        let plan = Plan {
            conflicts: vec!["src/main.rs".into()],
            ..Plan::default()
        };
        let error = refuse_conflicts(&plan, "archbox").unwrap_err().to_string();
        assert!(error.contains("  src/main.rs"));
        assert!(error.contains("Nothing was changed"));
    }
}
