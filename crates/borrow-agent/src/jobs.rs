//! Borrow runs and sessions on the Agent: persistent records, reconciliation after a
//! restart, persistent tmux sessions, and stopping work without trusting stale PIDs.

use crate::projects;
use anyhow::{Context, bail, ensure};
use borrow_core::control::{Job, JobKind, JobState, SessionInfo};
use borrow_core::preflight;
use borrow_core::storage;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Completed records kept for `borrow ps --all`.
const HISTORY: usize = 100;

/// How long stopped work gets to exit on its own before it is killed.
pub const GRACE: Duration = Duration::from_secs(5);

fn dir(root: &Path) -> PathBuf {
    root.join("jobs")
}

fn record(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    storage::valid_id(id)?;
    Ok(dir(root).join(format!("{id}.json")))
}

fn lock(root: &Path) -> anyhow::Result<fs::File> {
    storage::private_dir(&dir(root))?;
    storage::lock(&dir(root).join("records.lock"))
}

fn boot_time() -> u64 {
    System::boot_time()
}

/// Start time of a live process, or `None` when it is gone or already a zombie.
pub fn process_start(pid: u32) -> Option<u64> {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system
        .process(pid)
        .filter(|p| p.status() != sysinfo::ProcessStatus::Zombie)
        .map(|p| p.start_time())
}

pub fn save(root: &Path, job: &Job) -> anyhow::Result<()> {
    let _guard = lock(root)?;
    storage::write_json(&record(root, &job.id)?, job)
}

/// Change a record under the records lock so concurrent writers do not lose updates.
pub fn update(root: &Path, id: &str, change: impl FnOnce(&mut Job)) -> anyhow::Result<Job> {
    let _guard = lock(root)?;
    let file = record(root, id)?;
    let mut job: Job = serde_json::from_slice(&fs::read(&file)?)?;
    change(&mut job);
    storage::write_json(&file, &job)?;
    Ok(job)
}

pub fn new_job(kind: JobKind, project: Option<(&str, &str)>, command: String) -> Job {
    Job {
        id: storage::new_id(),
        kind,
        project: project.map(|(id, _)| id.to_string()),
        project_name: project.map(|(_, name)| name.to_string()),
        command,
        state: JobState::Running,
        started: storage::now(),
        ended: None,
        exit_code: None,
        pid: None,
        process_start: None,
        boot: boot_time(),
        stop_requested: false,
    }
}

/// Read every record, correcting any that claim to run but no longer do.
pub fn list(root: &Path, all: bool) -> anyhow::Result<Vec<Job>> {
    let _guard = lock(root)?;
    let mut jobs = Vec::new();
    for item in fs::read_dir(dir(root))? {
        let item = item?;
        let name = item.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if storage::valid_id(id).is_err() {
            continue;
        }
        let Ok(mut job) = serde_json::from_slice::<Job>(&fs::read(item.path())?) else {
            continue;
        };
        if job.state.active()
            && let Some(state) = reconcile(root, &job)
        {
            job.state = state;
            job.ended.get_or_insert_with(storage::now);
            storage::write_json(&item.path(), &job)?;
        }
        jobs.push(job);
    }
    jobs.sort_by(|a, b| b.started.cmp(&a.started).then(a.id.cmp(&b.id)));

    let mut finished = 0;
    let mut kept = Vec::new();
    for job in jobs {
        if job.state.active() {
            kept.push(job);
            continue;
        }
        finished += 1;
        if finished > HISTORY {
            let _ = fs::remove_file(record(root, &job.id)?);
        } else if all {
            kept.push(job);
        }
    }
    Ok(kept)
}

pub fn active_for(root: &Path, project: &str) -> anyhow::Result<Vec<Job>> {
    Ok(list(root, false)?
        .into_iter()
        .filter(|job| job.project.as_deref() == Some(project))
        .collect())
}

/// The state a running record should have, or `None` while it is really still running.
fn reconcile(root: &Path, job: &Job) -> Option<JobState> {
    if job.boot != boot_time() {
        return Some(JobState::Interrupted);
    }
    match job.kind {
        JobKind::Session if session_exists(root, &job.id) => None,
        JobKind::Session if job.stop_requested => Some(JobState::Stopped),
        JobKind::Session => Some(JobState::Ended),
        JobKind::Run => match (job.pid, job.process_start) {
            (Some(pid), Some(start)) if process_start(pid) == Some(start) => None,
            (None, _) if storage::now().saturating_sub(job.started) < 30 => None,
            _ if job.stop_requested => Some(JobState::Stopped),
            _ => Some(JobState::Interrupted),
        },
    }
}

/// Find an active job by its full ID or a unique prefix of at least four characters.
fn find_active(root: &Path, query: &str) -> anyhow::Result<Job> {
    ensure!(
        query.len() >= 4 && query.bytes().all(|b| b.is_ascii_hexdigit()),
        "Pass the job ID shown by borrow ps"
    );
    let query = query.to_ascii_lowercase();
    let matches: Vec<Job> = list(root, false)?
        .into_iter()
        .filter(|job| job.id.starts_with(&query))
        .collect();
    match matches.len() {
        0 => bail!("No running Borrow job matches {query}. See borrow ps"),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!("{query} matches several jobs. Use more characters"),
    }
}

fn tmux_socket(root: &Path) -> PathBuf {
    root.join("tmux").join("server")
}

/// The absolute tmux path, so attaching does not depend on the login shell's PATH.
fn tmux_program() -> Option<String> {
    let out = Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && path.starts_with('/')).then_some(path)
}

/// tmux on Borrow's own private socket, so personal tmux sessions are never touched.
fn tmux(root: &Path) -> Command {
    let mut command = Command::new("tmux");
    command.arg("-S").arg(tmux_socket(root));
    command
}

fn session_exists(root: &Path, id: &str) -> bool {
    tmux(root)
        .args(["has-session", "-t", &format!("={id}")])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Return the project's live session, or create one in the source copy.
pub fn session(root: &Path, project: &str) -> anyhow::Result<SessionInfo> {
    let (paths, metadata) = projects::load(root, project)?;
    let socket = tmux_socket(root)
        .to_str()
        .context("Agent storage path is not UTF 8")?
        .to_string();
    let program = tmux_program().with_context(|| {
        format!(
            "tmux is not installed on the Agent. Fix: {}",
            preflight::install_hint("tmux")
        )
    })?;
    let existing = |root: &Path| -> anyhow::Result<Option<Job>> {
        Ok(active_for(root, project)?
            .into_iter()
            .find(|job| job.kind == JobKind::Session))
    };
    if let Some(job) = existing(root)? {
        return Ok(SessionInfo {
            job,
            socket,
            tmux: program.clone(),
            created: false,
        });
    }
    let _lock = match projects::acquire_idle(root, &paths, &metadata) {
        Ok(lock) => lock,
        Err(error) => match existing(root)? {
            Some(job) => {
                return Ok(SessionInfo {
                    job,
                    socket,
                    tmux: program.clone(),
                    created: false,
                });
            }
            None => return Err(error),
        },
    };
    projects::ensure_ready(&paths, &metadata)?;
    let env = projects::prepare_artifacts(&paths)?;
    storage::private_dir(&root.join("tmux"))?;

    let job = new_job(
        JobKind::Session,
        Some((&metadata.id, &metadata.name)),
        "Shell session".to_string(),
    );
    save(root, &job)?;
    let mut command = tmux(root);
    command
        .args(["new-session", "-d", "-s", &job.id, "-c"])
        .arg(&paths.source);
    for (key, value) in env {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    let created = command.output();
    let failure = match &created {
        Ok(out) if out.status.success() => None,
        Ok(out) => Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(failure) = failure {
        let _ = update(root, &job.id, |job| {
            job.state = JobState::Failed;
            job.ended = Some(storage::now());
        });
        bail!("Could not start a tmux session: {failure}");
    }
    Ok(SessionInfo {
        job,
        socket,
        tmux: program,
        created: true,
    })
}

/// Stop a job. Work gets a graceful signal and five seconds, then anything still left
/// from the same process tree is killed. Every process is matched by PID and start time.
pub fn stop(root: &Path, query: &str) -> anyhow::Result<Job> {
    let job = find_active(root, query)?;
    update(root, &job.id, |job| job.stop_requested = true)?;
    match job.kind {
        JobKind::Run => stop_run(&job),
        JobKind::Session => stop_session(root, &job),
    }
    let _ = list(root, false);
    update(root, &job.id, |job| {
        if job.state.active() {
            job.state = JobState::Stopped;
            job.ended = Some(storage::now());
        }
    })
}

fn stop_run(job: &Job) {
    let (Some(pid), Some(start)) = (job.pid, job.process_start) else {
        return;
    };
    let tree = process_tree(&[pid], Some(pid));
    if process_start(pid) == Some(start) {
        signal_group(pid, start, libc::SIGINT);
    }
    wait_for_exit(&tree, GRACE);
    kill_remaining(&tree);
}

fn stop_session(root: &Path, job: &Job) {
    let target = format!("={}", job.id);
    let panes: Vec<u32> = tmux(root)
        .args(["list-panes", "-s", "-t", &target, "-F", "#{pane_pid}"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let tree = process_tree(&panes, None);
    let shells: HashMap<u32, u64> = tree
        .iter()
        .filter(|(pid, _)| panes.contains(pid))
        .map(|(pid, start)| (*pid, *start))
        .collect();
    let work: HashMap<u32, u64> = tree
        .iter()
        .filter(|(pid, _)| !shells.contains_key(pid))
        .map(|(pid, start)| (*pid, *start))
        .collect();

    let _ = tmux(root)
        .args(["list-panes", "-s", "-t", &target, "-F", "#{pane_id}"])
        .output()
        .map(|out| {
            for pane in String::from_utf8_lossy(&out.stdout).lines() {
                let _ = tmux(root)
                    .args(["send-keys", "-t", pane.trim(), "C-c"])
                    .output();
            }
        });
    wait_for_exit(&work, GRACE);
    let _ = tmux(root).args(["kill-session", "-t", &target]).output();
    wait_for_exit(&tree, Duration::from_secs(1));
    kill_remaining(&tree);
}

/// Live processes that belong to the given roots: their descendants, and members of the
/// process group led by `group`. Recorded with start times so reused PIDs are ignored.
fn process_tree(roots: &[u32], group: Option<u32>) -> HashMap<u32, u64> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Never),
    );
    let mut tree: HashMap<u32, u64> = HashMap::new();
    for pid in roots {
        if let Some(process) = system.process(Pid::from_u32(*pid)) {
            tree.insert(*pid, process.start_time());
        }
    }
    loop {
        let before = tree.len();
        for (pid, process) in system.processes() {
            let pid = pid.as_u32();
            if tree.contains_key(&pid) {
                continue;
            }
            let child = process
                .parent()
                .is_some_and(|parent| tree.contains_key(&parent.as_u32()));
            let member = group.is_some_and(|g| unsafe { libc::getpgid(pid as i32) } == g as i32);
            if child || member {
                tree.insert(pid, process.start_time());
            }
        }
        if tree.len() == before {
            break;
        }
    }
    tree
}

fn wait_for_exit(processes: &HashMap<u32, u64>, limit: Duration) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if processes
            .iter()
            .all(|(pid, start)| process_start(*pid) != Some(*start))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn kill_remaining(processes: &HashMap<u32, u64>) {
    for (pid, start) in processes {
        if process_start(*pid) == Some(*start) {
            unsafe {
                libc::kill(*pid as i32, libc::SIGKILL);
            }
        }
    }
}

/// Signal a process group only while its leader still has the recorded start time.
pub fn signal_group(pid: u32, start: u64, signal: i32) -> bool {
    if pid <= 1 || process_start(pid) != Some(start) {
        return false;
    }
    unsafe { libc::kill(-(pid as i32), signal) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Root;

    fn running(root: &Path, pid: Option<u32>, start: Option<u64>) -> Job {
        let mut job = new_job(JobKind::Run, None, "sleep".into());
        job.pid = pid;
        job.process_start = start;
        save(root, &job).unwrap();
        job
    }

    #[test]
    fn a_record_for_a_vanished_process_is_marked_interrupted() {
        let root = Root::new();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let start = process_start(pid);
        child.wait().unwrap();
        let job = running(&root.0, Some(pid), start.or(Some(1)));
        let jobs = list(&root.0, true).unwrap();
        assert_eq!(jobs[0].id, job.id);
        assert_eq!(jobs[0].state, JobState::Interrupted);
        assert!(list(&root.0, false).unwrap().is_empty());
    }

    #[test]
    fn a_reused_pid_with_a_different_start_time_is_not_trusted() {
        let root = Root::new();
        let me = std::process::id();
        let real = process_start(me).unwrap();
        let job = running(&root.0, Some(me), Some(real + 1000));
        assert!(!signal_group(me, real + 1000, 0));
        assert_eq!(list(&root.0, true).unwrap()[0].state, JobState::Interrupted);
        assert!(find_active(&root.0, &job.id).is_err());
    }

    #[test]
    fn a_record_from_before_a_reboot_is_interrupted() {
        let root = Root::new();
        let me = std::process::id();
        let mut job = running(&root.0, Some(me), process_start(me));
        assert_eq!(list(&root.0, false).unwrap().len(), 1);
        job.boot = 1;
        save(&root.0, &job).unwrap();
        assert_eq!(list(&root.0, true).unwrap()[0].state, JobState::Interrupted);
    }

    #[test]
    fn stopping_a_run_signals_its_whole_group_and_records_it() {
        use std::os::unix::process::CommandExt;
        let root = Root::new();
        let mut child = std::process::Command::new("sh")
            .args(["-c", "trap '' INT; sleep 30 & sleep 30; wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(200));
        let job = running(&root.0, Some(pid), process_start(pid));
        let tree = process_tree(&[pid], Some(pid));
        assert!(tree.len() >= 3, "{tree:?}");
        let started = Instant::now();
        let stopped = stop(&root.0, storage::short_id(&job.id)).unwrap();
        let _ = child.wait();
        assert_eq!(stopped.state, JobState::Stopped);
        assert!(started.elapsed() >= GRACE - Duration::from_millis(200));
        assert!(
            tree.iter()
                .all(|(pid, start)| process_start(*pid) != Some(*start))
        );
    }

    #[test]
    fn prefixes_must_be_unique_and_well_formed() {
        let root = Root::new();
        let me = std::process::id();
        let job = running(&root.0, Some(me), process_start(me));
        assert_eq!(
            find_active(&root.0, storage::short_id(&job.id)).unwrap().id,
            job.id
        );
        assert!(find_active(&root.0, "ab").is_err());
        assert!(find_active(&root.0, "../x").is_err());
    }

    #[test]
    fn history_keeps_the_most_recent_hundred_finished_jobs() {
        let root = Root::new();
        for index in 0..105 {
            let mut job = new_job(JobKind::Run, None, format!("job {index}"));
            job.state = JobState::Completed;
            job.started = 1000 + index;
            save(&root.0, &job).unwrap();
        }
        let jobs = list(&root.0, true).unwrap();
        assert_eq!(jobs.len(), HISTORY);
        assert_eq!(jobs[0].command, "job 104");
        assert_eq!(fs::read_dir(dir(&root.0)).unwrap().count(), HISTORY + 1);
    }
}
