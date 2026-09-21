//! Pairing messages, sent as newline separated JSON over TCP. Everything after pairing
//! uses the authenticated control channel in `control`.

use serde::{Deserialize, Serialize};

/// Default Agent control port.
pub const DEFAULT_PORT: u16 = 7433;

/// What the Client asks for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Retired. Older Clients sent these unauthenticated; the daemon now answers with
    /// an instruction to update.
    Info,
    Health,
    /// Trade a pairing token for an installed key. Single use. `user` and `host_keys`
    /// describe the Client and are kept for compatibility with older Agents.
    Pair {
        token: String,
        client: String,
        public_key: String,
        user: String,
        host_keys: Vec<String>,
    },
}

/// What the Agent answers. Pairing is all this port does, so the only outcomes are a
/// successful pairing and an `Error` carrying a sentence meant for a human.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    /// Boxed because it carries the box's full specs, which dwarf an error message.
    Paired(Box<Paired>),
    Error {
        message: String,
    },
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
    /// The Agent's Borrow program path, so the Client can start helpers over SSH.
    pub program: Option<String>,
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
    pub memory_mib: u64,
    pub disk_total_mib: u64,
    /// Tooling that was found on the box, such as docker or tmux.
    pub tools: Vec<String>,
    /// Last, because toml cannot put a plain value after a list of tables.
    pub gpus: Vec<Gpu>,
}

/// A graphics card as reported at pairing time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    pub vram_mib: Option<u64>,
}

/// Current resource usage and available capacity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub cpu_percent: f32,
    pub memory_used_mib: u64,
    pub memory_total_mib: u64,
    pub swap_total_mib: u64,
    pub disk_free_mib: u64,
    /// Free space where Borrow keeps project copies and build output.
    #[serde(default)]
    pub workspace_free_mib: Option<u64>,
    pub gpus: Vec<GpuHealth>,
    /// Why GPU numbers are missing when nvidia-smi is installed but failing.
    #[serde(default)]
    pub gpu_problem: Option<String>,
}

/// Live GPU numbers. VRAM is reported as free rather than total, because a
/// desktop session holds a few hundred megabytes even when the box looks idle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuHealth {
    pub name: String,
    pub vram_free_mib: Option<u64>,
    pub vram_total_mib: Option<u64>,
    pub utilization_percent: Option<u32>,
    pub temperature_c: Option<u32>,
}
