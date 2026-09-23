//! Deciding when something is worth a notification. Each observation of the box goes in,
//! and the notices it earns come out, so the rules can be tested without a box.

use super::event::{Notice, NoticeKind};
use crate::commands::health::{MEMORY, TEMPERATURE};
use slingshot_core::control::{Job, JobKind, JobState};
use slingshot_core::presentation::capacity;
use slingshot_core::protocol::Health;
use slingshot_core::step;
use slingshot_core::telemetry::DISK_WARNING_MIB;
use std::collections::HashSet;
use std::time::Duration;

/// A run shorter than this finished while you were still looking at it.
const WORTH_NOTICING: u64 = 10;

/// Failed checks in a row before the box counts as unreachable, so one slow answer is not
/// reported as an outage.
const FAILURES_BEFORE_UNREACHABLE: u32 = 2;

/// Free workspace space must recover this far past the warning before it can warn again.
const DISK_RECOVERED_MIB: u64 = DISK_WARNING_MIB * 2;

const COMMAND_WIDTH: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Alarm {
    Memory,
    Workspace,
    Temperature(usize),
}

pub struct Watch {
    name: String,
    /// Finished jobs already accounted for. Empty until the first answer sets the baseline.
    known: Option<HashSet<String>>,
    failures: u32,
    unreachable: bool,
    alarms: HashSet<Alarm>,
}

impl Watch {
    pub fn new(name: &str) -> Watch {
        Watch {
            name: name.to_string(),
            known: None,
            failures: 0,
            unreachable: false,
            alarms: HashSet::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Failed checks since the box last answered.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// The box answered.
    pub fn reached(&mut self) -> Vec<Notice> {
        self.failures = 0;
        if !std::mem::take(&mut self.unreachable) {
            return Vec::new();
        }
        vec![Notice {
            kind: NoticeKind::Back,
            title: format!("{} is back", self.name),
            body: "Slingshot can reach it again".to_string(),
        }]
    }

    /// The box did not answer, for the reason in `error`.
    pub fn failed(&mut self, error: &str) -> Vec<Notice> {
        self.failures += 1;
        if self.unreachable || self.failures < FAILURES_BEFORE_UNREACHABLE {
            return Vec::new();
        }
        self.unreachable = true;
        vec![Notice {
            kind: NoticeKind::Unreachable,
            title: format!("{} is unreachable", self.name),
            body: error.to_string(),
        }]
    }

    /// Jobs that finished since the last list. The first list only sets the baseline, so
    /// starting the menu bar never replays history.
    pub fn jobs(&mut self, jobs: &[Job]) -> Vec<Notice> {
        let finished: Vec<&Job> = jobs.iter().filter(|job| !job.state.active()).collect();
        let Some(known) = &mut self.known else {
            self.known = Some(finished.iter().map(|job| job.id.clone()).collect());
            return Vec::new();
        };
        let notices = finished
            .iter()
            .filter(|job| known.insert(job.id.clone()))
            .filter_map(|job| notice(&self.name, job))
            .collect();
        let listed: HashSet<&str> = jobs.iter().map(|job| job.id.as_str()).collect();
        known.retain(|id| listed.contains(id.as_str()));
        notices
    }

    /// Resources that just crossed into trouble. Each warns once, then again only after
    /// it has clearly recovered.
    pub fn health(&mut self, health: &Health) -> Vec<Notice> {
        let mut notices = Vec::new();
        let (used, total) = (health.memory_used_mib, health.memory_total_mib);
        if total > 0 && used <= total {
            let percent = used as f64 / total as f64 * 100.0;
            if self.alarm(
                Alarm::Memory,
                percent >= MEMORY.high,
                percent < MEMORY.warning,
            ) {
                notices.push(resource(
                    format!("{} is low on memory", self.name),
                    format!("{} of {} RAM in use", capacity(used), capacity(total)),
                ));
            }
        }
        let free = health.workspace_free_mib.unwrap_or(health.disk_free_mib);
        if self.alarm(
            Alarm::Workspace,
            free < DISK_WARNING_MIB,
            free >= DISK_RECOVERED_MIB,
        ) {
            notices.push(resource(
                format!("{} is low on disk space", self.name),
                format!(
                    "{} free for project copies and build output",
                    capacity(free)
                ),
            ));
        }
        for (index, gpu) in health.gpus.iter().enumerate() {
            let Some(celsius) = gpu.temperature_c else {
                continue;
            };
            let value = celsius as f64;
            if self.alarm(
                Alarm::Temperature(index),
                value >= TEMPERATURE.high,
                value < TEMPERATURE.warning,
            ) {
                notices.push(resource(
                    format!("{}'s GPU is hot", self.name),
                    format!("{} at {celsius}°C", gpu.name),
                ));
            }
        }
        notices
    }

    /// True when `alarm` should fire now. It re-arms only once `recovered` holds.
    fn alarm(&mut self, alarm: Alarm, triggered: bool, recovered: bool) -> bool {
        if recovered {
            self.alarms.remove(&alarm);
            return false;
        }
        triggered && self.alarms.insert(alarm)
    }
}

fn resource(title: String, body: String) -> Notice {
    Notice {
        kind: NoticeKind::Resource,
        title,
        body,
    }
}

/// What a finished job is worth saying. Stopping is something the person just did, and a
/// session usually ends because someone left it, so only surprises about those are told.
fn notice(name: &str, job: &Job) -> Option<Notice> {
    let took = job.ended?.saturating_sub(job.started);
    let command = short(&job.command);
    let details = |first: Option<String>| {
        first
            .into_iter()
            .chain(job.project_name.clone())
            .chain(Some(step::elapsed(Duration::from_secs(took))))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    match (job.kind, job.state) {
        (_, JobState::Interrupted) => Some(Notice {
            kind: NoticeKind::JobInterrupted,
            title: format!("{command} was interrupted on {name}"),
            body: details(Some("Connection lost or the box restarted".to_string())),
        }),
        (JobKind::Run, _) if took < WORTH_NOTICING => None,
        (JobKind::Run, JobState::Completed) => Some(Notice {
            kind: NoticeKind::JobFinished,
            title: format!("✓ {command} finished on {name}"),
            body: details(None),
        }),
        (JobKind::Run, JobState::Failed) => Some(Notice {
            kind: NoticeKind::JobFailed,
            title: format!("✗ {command} failed on {name}"),
            body: details(job.exit_code.map(|code| format!("exit {code}"))),
        }),
        _ => None,
    }
}

fn short(command: &str) -> String {
    match command.chars().count() > COMMAND_WIDTH {
        true => format!(
            "{}…",
            command.chars().take(COMMAND_WIDTH - 1).collect::<String>()
        ),
        false => command.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slingshot_core::protocol::GpuHealth;

    fn job(id: &str, kind: JobKind, state: JobState, took: u64) -> Job {
        Job {
            id: id.to_string(),
            kind,
            project: Some("p".to_string()),
            project_name: Some("app".to_string()),
            command: "cargo build --release".to_string(),
            state,
            started: 1_000,
            ended: (!state.active()).then_some(1_000 + took),
            exit_code: match state {
                JobState::Completed => Some(0),
                JobState::Failed => Some(101),
                _ => None,
            },
            pid: None,
            process_start: None,
            boot: 0,
            stop_requested: false,
        }
    }

    fn run(id: &str, state: JobState, took: u64) -> Job {
        job(id, JobKind::Run, state, took)
    }

    fn health(memory_used_mib: u64, workspace_free_mib: u64, celsius: u32) -> Health {
        Health {
            cpu_percent: 10.0,
            memory_used_mib,
            memory_total_mib: 100,
            swap_total_mib: 0,
            disk_free_mib: 0,
            workspace_free_mib: Some(workspace_free_mib),
            gpus: vec![GpuHealth {
                name: "RTX 3090".to_string(),
                vram_free_mib: None,
                vram_total_mib: None,
                utilization_percent: None,
                temperature_c: Some(celsius),
            }],
            gpu_problem: None,
        }
    }

    const PLENTY: u64 = 100_000;

    #[test]
    fn the_first_list_sets_a_baseline() {
        let mut watch = Watch::new("archbox");
        let old = [
            run("a", JobState::Completed, 60),
            run("b", JobState::Failed, 60),
        ];
        assert!(watch.jobs(&old).is_empty());
        assert!(watch.jobs(&old).is_empty());
    }

    #[test]
    fn a_long_run_that_ends_is_told_once() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[run("a", JobState::Running, 0)]);
        let notices = watch.jobs(&[run("a", JobState::Completed, 192)]);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].kind, NoticeKind::JobFinished);
        assert_eq!(
            notices[0].title,
            "✓ cargo build --release finished on archbox"
        );
        assert_eq!(notices[0].body, "app · 3m 12s");
        assert!(watch.jobs(&[run("a", JobState::Completed, 192)]).is_empty());
    }

    #[test]
    fn a_failure_names_the_exit_code() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let notices = watch.jobs(&[run("a", JobState::Failed, 12)]);
        assert_eq!(notices[0].kind, NoticeKind::JobFailed);
        assert_eq!(
            notices[0].title,
            "✗ cargo build --release failed on archbox"
        );
        assert_eq!(notices[0].body, "exit 101 · app · 12s");
    }

    #[test]
    fn short_and_stopped_runs_and_ended_sessions_stay_quiet() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        assert!(
            watch
                .jobs(&[
                    run("a", JobState::Completed, 9),
                    run("b", JobState::Failed, 2),
                    run("c", JobState::Stopped, 600),
                    job("d", JobKind::Session, JobState::Ended, 600),
                ])
                .is_empty()
        );
    }

    #[test]
    fn interruptions_are_always_told() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let notices = watch.jobs(&[
            job("a", JobKind::Session, JobState::Interrupted, 3),
            run("b", JobState::Interrupted, 1),
        ]);
        assert_eq!(notices.len(), 2);
        assert!(
            notices
                .iter()
                .all(|notice| notice.kind == NoticeKind::JobInterrupted)
        );
        assert!(notices[0].title.ends_with("was interrupted on archbox"));
    }

    #[test]
    fn long_commands_are_shortened() {
        let long = "x".repeat(60);
        let shortened = short(&long);
        assert_eq!(shortened.chars().count(), COMMAND_WIDTH);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn unreachable_needs_two_failures_and_is_told_once_then_back() {
        let mut watch = Watch::new("archbox");
        assert!(watch.reached().is_empty());
        assert!(watch.failed("timed out").is_empty());
        let notices = watch.failed("timed out");
        assert_eq!(notices[0].kind, NoticeKind::Unreachable);
        assert_eq!(notices[0].title, "archbox is unreachable");
        assert_eq!(notices[0].body, "timed out");
        assert!(watch.failed("timed out").is_empty());
        let notices = watch.reached();
        assert_eq!(notices[0].kind, NoticeKind::Back);
        assert!(watch.reached().is_empty());
    }

    #[test]
    fn one_failure_between_answers_is_not_an_outage() {
        let mut watch = Watch::new("archbox");
        watch.failed("slow");
        watch.reached();
        assert!(watch.failed("slow").is_empty());
    }

    #[test]
    fn resources_warn_once_and_rearm_after_recovering() {
        let mut watch = Watch::new("archbox");
        assert!(watch.health(&health(50, PLENTY, 60)).is_empty());
        let notices = watch.health(&health(95, PLENTY, 60));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "archbox is low on memory");
        assert!(watch.health(&health(95, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(80, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(92, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(70, PLENTY, 60)).is_empty());
        assert_eq!(watch.health(&health(91, PLENTY, 60)).len(), 1);
    }

    #[test]
    fn disk_and_temperature_warn_with_their_own_limits() {
        let mut watch = Watch::new("archbox");
        let notices = watch.health(&health(10, 1024, 90));
        let titles: Vec<_> = notices.iter().map(|notice| notice.title.as_str()).collect();
        assert_eq!(
            titles,
            ["archbox is low on disk space", "archbox's GPU is hot"]
        );
        assert!(watch.health(&health(10, DISK_WARNING_MIB, 80)).is_empty());
        assert!(watch.health(&health(10, 1024, 90)).is_empty());
        assert!(watch.health(&health(10, DISK_RECOVERED_MIB, 70)).is_empty());
        assert_eq!(watch.health(&health(10, 1024, 90)).len(), 2);
    }
}
