//! The small set of messages the Client and the Agent exchange.
//!
//! One request, one response, newline delimited JSON over TCP. The daemon is the
//! control plane, so these messages carry facts about the box and never carry work.

use serde::{Deserialize, Serialize};

/// The port the daemon listens on by default. Picked to sit well clear of the
/// common ranges so it rarely collides with anything already running.
pub const DEFAULT_PORT: u16 = 7433;

/// What the Client asks for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Static facts about the box. Fetched once at pairing and cached.
    Info,
    /// What the box is doing right now.
    Health,
    /// Trade a pairing token for an installed key. Single use.
    Pair {
        token: String,
        client: String,
        public_key: String,
    },
}

/// What the Agent answers. `Error` carries a sentence meant for a human.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Info(Specs),
    Health(Health),
    Paired(Paired),
    Error { message: String },
}

/// Everything `link` needs to be able to reach the box from now on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paired {
    /// The display name chosen on the Agent. Config stores this so the
    /// "running on" line stays stable even if the machine gets renamed.
    pub name: String,
    /// The account the installed key belongs to, so ssh knows who to log in as.
    pub user: String,
    /// The box's ssh host keys, so the Client can recognise it later without
    /// anybody being asked to eyeball a fingerprint.
    pub host_keys: Vec<String>,
    pub specs: Specs,
}

/// Static facts. These change only when hardware or the OS changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Specs {
    pub name: String,
    pub os: String,
    pub kernel: String,
    pub cpu: String,
    pub cores: usize,
    pub memory_mb: u64,
    pub disk_total_mb: u64,
    /// Tooling that was found on the box, such as docker or sshfs.
    pub tools: Vec<String>,
    /// Last, because toml cannot put a plain value after a list of tables.
    pub gpus: Vec<Gpu>,
}

/// A graphics card as reported at pairing time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: Option<u64>,
}

/// A live snapshot. Every number here is the sort that would make somebody
/// cancel a job, so free and available are reported rather than totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub swap_total_mb: u64,
    pub disk_free_mb: u64,
    pub gpus: Vec<GpuHealth>,
}

/// Live GPU numbers. VRAM is reported as free rather than total, because a
/// desktop session holds a few hundred megabytes even when the box looks idle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuHealth {
    pub name: String,
    pub vram_free_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub utilization_percent: Option<u32>,
    pub temperature_c: Option<u32>,
}
