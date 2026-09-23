//! `slingshot ps` and `slingshot stop`: Slingshot runs and sessions on the Agent.

use crate::client::{self, unexpected};
use slingshot_core::config::Config;
use slingshot_core::control::{Job, JobKind, JobState, Request, Response};
use slingshot_core::presentation::{self, Style, Tone};
use slingshot_core::storage;

pub async fn ps(agent: Option<String>, all: bool) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let Response::Jobs(jobs) = client::request(target, Request::Jobs { all }).await? else {
        return Err(unexpected());
    };
    print!(
        "{}",
        render(&target.name, &jobs, all, storage::now(), Style::stdout())
    );
    Ok(0)
}

pub async fn stop(agent: Option<String>, id: String) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    presentation::progress(format!("Stopping {id} on {}", target.name));
    let Response::Job(job) = client::request(target, Request::Stop { job: id }).await? else {
        return Err(unexpected());
    };
    presentation::success(format!(
        "Stopped {} ({}). Source and build output were kept",
        job.command,
        storage::short_id(&job.id)
    ));
    Ok(0)
}

pub fn render(agent: &str, jobs: &[Job], all: bool, now: u64, style: Style) -> String {
    let heading = match all {
        true => format!("Slingshot jobs on {agent}"),
        false => format!("Active Slingshot jobs on {agent}"),
    };
    let mut output = format!("{}\n", style.heading(heading));
    if jobs.is_empty() {
        output.push_str(match all {
            true => "  No jobs recorded\n",
            false => "  Nothing running. See finished jobs with slingshot ps --all\n",
        });
        return output;
    }
    output.push_str(&format!(
        "  {:<8}  {:<7}  {:<16}  {:<16}  {:<9}  {}\n",
        "ID", "KIND", "STATE", "PROJECT", "STARTED", "COMMAND"
    ));
    for job in jobs {
        let kind = match job.kind {
            JobKind::Run => "Run",
            JobKind::Session => "Session",
        };
        let tone = match job.state {
            JobState::Running => Tone::Good,
            JobState::Completed | JobState::Ended => Tone::Info,
            JobState::Stopped | JobState::Interrupted => Tone::Warning,
            JobState::Failed => Tone::Error,
        };
        let mut state = job.state.label().to_string();
        if let Some(code) = job.exit_code.filter(|_| job.state == JobState::Failed) {
            state = format!("{state} {code}");
        }
        let project: String = job
            .project_name
            .clone()
            .unwrap_or_else(|| "none".to_string())
            .chars()
            .take(16)
            .collect();
        output.push_str(&format!(
            "  {:<8}  {:<7}  {}  {:<16}  {:<9}  {}\n",
            storage::short_id(&job.id),
            kind,
            style.paint(format!("{state:<16}"), tone),
            project,
            ago(now.saturating_sub(job.started)),
            job.command
        ));
    }
    output
}

fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s ago"),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: JobKind, state: JobState, started: u64) -> Job {
        Job {
            id: "1a2b3c4d5e6f77889900aabbccddeeff".into(),
            kind,
            project: Some("p".into()),
            project_name: Some("app".into()),
            command: "cargo build --release".into(),
            state,
            started,
            ended: None,
            exit_code: None,
            pid: None,
            process_start: None,
            boot: 0,
            stop_requested: false,
        }
    }

    #[test]
    fn jobs_are_listed_with_short_ids_and_readable_ages() {
        let mut failed = job(JobKind::Run, JobState::Failed, 1000);
        failed.exit_code = Some(101);
        let jobs = vec![job(JobKind::Session, JobState::Running, 9_950), failed];
        let text = render("archbox", &jobs, true, 10_000, Style::new(false));
        assert!(text.contains("Slingshot jobs on archbox"));
        assert!(text.contains(
            "1a2b3c4d  Session  Running           app               50s ago    cargo build --release"
        ));
        assert!(text.contains("Failed 101"));
        assert!(text.contains("2h ago"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn only_failures_show_their_exit_code() {
        let mut interrupted = job(JobKind::Run, JobState::Interrupted, 1000);
        interrupted.exit_code = Some(130);
        let mut stopped = job(JobKind::Run, JobState::Stopped, 1000);
        stopped.exit_code = Some(130);
        let text = render(
            "archbox",
            &[interrupted, stopped],
            true,
            1010,
            Style::new(false),
        );
        assert!(text.contains("Interrupted       app"), "{text}");
        assert!(text.contains("Stopped           app"), "{text}");
        assert!(!text.contains("130"), "{text}");
    }

    #[test]
    fn an_empty_list_explains_itself() {
        let text = render("archbox", &[], false, 0, Style::new(false));
        assert!(text.contains("Nothing running. See finished jobs with slingshot ps --all"));
    }
}
