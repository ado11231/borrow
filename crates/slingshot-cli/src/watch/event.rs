//! The lines `slingshot internal-watch` prints for the menu bar app. Each line is one JSON
//! object. The app only draws and posts what arrives here, so every threshold and every
//! decision to notify stays in Rust.

use crate::commands::health::{LOAD, Limits, MEMORY, TEMPERATURE};
use serde::Serialize;
use slingshot_core::presentation::Tone;
use slingshot_core::protocol::Health;
use slingshot_core::telemetry::DISK_WARNING_MIB;

/// Raised whenever a field changes meaning or disappears, so the app can ask for an update
/// rather than draw something wrong.
pub const VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct Line<'a> {
    pub version: u32,
    #[serde(flatten)]
    pub event: &'a Event,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Status(Status),
    Notice(Notice),
}

/// Everything the popover shows. Measurements are absent while the box is offline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub agent: String,
    pub online: bool,
    /// The path in use, as in "via tailnet".
    pub path: Option<String>,
    pub error: Option<String>,
    pub cpu: Option<Percent>,
    pub memory: Option<Usage>,
    pub workspace: Option<Space>,
    pub gpus: Vec<Gpu>,
    pub gpu_problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Percent {
    pub percent: f32,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    pub used_mib: u64,
    pub total_mib: u64,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Space {
    pub free_mib: u64,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gpu {
    pub name: String,
    pub utilization: Option<Percent>,
    pub temperature_c: Option<u32>,
    pub temperature_level: Option<Level>,
    pub vram: Option<Usage>,
}

/// How much attention a value needs. Healthy values are drawn plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Good,
    Warning,
    High,
}

impl Level {
    fn of(value: f64, limits: Limits) -> Level {
        match limits.tone(value) {
            Tone::Error => Level::High,
            Tone::Warning => Level::Warning,
            _ => Level::Good,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notice {
    pub kind: NoticeKind,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    JobFinished,
    JobFailed,
    JobInterrupted,
    Unreachable,
    Back,
    Resource,
}

impl Status {
    pub fn online(agent: &str, path: &str, health: &Health) -> Status {
        Status {
            agent: agent.to_string(),
            online: true,
            path: Some(path.to_string()),
            error: None,
            cpu: percent(health.cpu_percent as f64, LOAD),
            memory: usage(health.memory_used_mib, health.memory_total_mib),
            workspace: Some(space(
                health.workspace_free_mib.unwrap_or(health.disk_free_mib),
            )),
            gpus: health.gpus.iter().map(gpu).collect(),
            gpu_problem: health.gpu_problem.clone(),
        }
    }

    pub fn offline(agent: &str, error: String) -> Status {
        Status {
            agent: agent.to_string(),
            online: false,
            path: None,
            error: Some(error),
            cpu: None,
            memory: None,
            workspace: None,
            gpus: Vec::new(),
            gpu_problem: None,
        }
    }
}

fn percent(value: f64, limits: Limits) -> Option<Percent> {
    (value.is_finite() && (0.0..=100.0).contains(&value)).then(|| Percent {
        percent: value as f32,
        level: Level::of(value, limits),
    })
}

fn usage(used_mib: u64, total_mib: u64) -> Option<Usage> {
    (total_mib > 0 && used_mib <= total_mib).then(|| Usage {
        used_mib,
        total_mib,
        level: Level::of(used_mib as f64 / total_mib as f64 * 100.0, MEMORY),
    })
}

fn space(free_mib: u64) -> Space {
    Space {
        free_mib,
        level: match free_mib < DISK_WARNING_MIB {
            true => Level::High,
            false => Level::Good,
        },
    }
}

fn gpu(gpu: &slingshot_core::protocol::GpuHealth) -> Gpu {
    Gpu {
        name: gpu.name.clone(),
        utilization: gpu
            .utilization_percent
            .and_then(|value| percent(value as f64, LOAD)),
        temperature_c: gpu.temperature_c,
        temperature_level: gpu
            .temperature_c
            .map(|value| Level::of(value as f64, TEMPERATURE)),
        vram: match (gpu.vram_free_mib, gpu.vram_total_mib) {
            (Some(free), Some(total)) if free <= total => usage(total - free, total),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slingshot_core::protocol::GpuHealth;

    fn health() -> Health {
        Health {
            cpu_percent: 40.0,
            memory_used_mib: 30000,
            memory_total_mib: 32768,
            swap_total_mib: 4096,
            disk_free_mib: 500_000,
            workspace_free_mib: Some(1024),
            gpus: vec![GpuHealth {
                name: "RTX 3090".to_string(),
                vram_free_mib: Some(15000),
                vram_total_mib: Some(24576),
                utilization_percent: Some(75),
                temperature_c: Some(60),
            }],
            gpu_problem: None,
        }
    }

    #[test]
    fn a_status_line_carries_values_and_levels() {
        let event = Event::Status(Status::online("archbox", "tailnet", &health()));
        let line = serde_json::to_value(Line {
            version: VERSION,
            event: &event,
        })
        .unwrap();
        assert_eq!(line["version"], 1);
        assert_eq!(line["event"], "status");
        assert_eq!(line["online"], true);
        assert_eq!(line["path"], "tailnet");
        assert_eq!(line["cpu"]["level"], "good");
        assert_eq!(line["memory"]["level"], "high");
        assert_eq!(line["workspace"]["free_mib"], 1024);
        assert_eq!(line["workspace"]["level"], "high");
        assert_eq!(line["gpus"][0]["utilization"]["level"], "warning");
        assert_eq!(line["gpus"][0]["temperature_level"], "good");
        assert_eq!(line["gpus"][0]["vram"]["used_mib"], 24576 - 15000);
    }

    #[test]
    fn invalid_measurements_are_left_out() {
        let mut health = health();
        health.cpu_percent = f32::NAN;
        health.memory_total_mib = 0;
        health.workspace_free_mib = None;
        health.gpus[0].vram_free_mib = Some(99_999);
        let status = Status::online("archbox", "iroh", &health);
        assert!(status.cpu.is_none());
        assert!(status.memory.is_none());
        assert_eq!(status.workspace.unwrap().free_mib, 500_000);
        assert!(status.gpus[0].vram.is_none());
    }

    #[test]
    fn a_notice_line_is_flat() {
        let event = Event::Notice(Notice {
            kind: NoticeKind::JobFinished,
            title: "t".to_string(),
            body: "b".to_string(),
        });
        let text = serde_json::to_string(&Line {
            version: VERSION,
            event: &event,
        })
        .unwrap();
        assert_eq!(
            text,
            r#"{"version":1,"event":"notice","kind":"job_finished","title":"t","body":"b"}"#
        );
    }
}
