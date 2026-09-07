//! Reading the box's specs and health.
//!
//! Everything here runs on the Agent. Collection is cheap on purpose: sysinfo for
//! CPU, memory and disk, nvidia-smi for GPU. A box with no GPU is normal, not an
//! error, so the GPU list simply comes back empty.

use crate::protocol::{Gpu, GpuHealth, Health, Specs};
use std::process::Command;
use sysinfo::{Disks, System, MINIMUM_CPU_UPDATE_INTERVAL};

/// Programs worth knowing about when deciding what the box can do for you.
const INTERESTING_TOOLS: &[&str] = &["docker", "podman", "sshfs", "rsync", "git", "tmux", "ollama", "nvidia-smi"];

const BYTES_PER_MB: u64 = 1024 * 1024;

/// Static facts about the box. `name` is passed in rather than read from the machine,
/// because it is chosen at pairing and should not drift if the hostname changes.
pub fn specs(name: &str) -> Specs {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "unknown cpu".to_string());

    Specs {
        name: name.to_string(),
        os: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
        kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        cpu,
        cores: sys.cpus().len(),
        memory_mb: sys.total_memory() / BYTES_PER_MB,
        disk_total_mb: root_disk().map(|(total, _)| total).unwrap_or(0) / BYTES_PER_MB,
        gpus: gpu_specs(),
        tools: INTERESTING_TOOLS
            .iter()
            .filter(|tool| is_installed(tool))
            .map(|tool| tool.to_string())
            .collect(),
    }
}

/// A live snapshot. CPU usage needs two samples with a gap between them, because a
/// percentage is a change over time and a single reading has nothing to compare to.
pub fn health() -> Health {
    let mut sys = System::new_all();
    sys.refresh_cpu_usage();
    std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    Health {
        cpu_percent: sys.global_cpu_usage(),
        memory_used_mb: sys.used_memory() / BYTES_PER_MB,
        memory_total_mb: sys.total_memory() / BYTES_PER_MB,
        swap_total_mb: sys.total_swap() / BYTES_PER_MB,
        disk_free_mb: root_disk().map(|(_, free)| free).unwrap_or(0) / BYTES_PER_MB,
        gpus: gpu_health(),
    }
}

/// Total and available bytes for the filesystem holding the root of the tree.
/// That is the one that fills up and kills a build, so it is the one worth showing.
fn root_disk() -> Option<(u64, u64)> {
    let disks = Disks::new_with_refreshed_list();

    let root = disks
        .list()
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))
        .or_else(|| disks.list().iter().max_by_key(|d| d.total_space()))?;

    Some((root.total_space(), root.available_space()))
}

/// True when a program can be found on PATH.
pub fn is_installed(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Ask nvidia-smi for comma separated values. Returns an empty list when there is
/// no Nvidia GPU, which is the ordinary Linux to Linux case rather than a failure.
fn nvidia_smi(fields: &str) -> Vec<Vec<String>> {
    let Ok(out) = Command::new("nvidia-smi")
        .arg(format!("--query-gpu={fields}"))
        .arg("--format=csv,noheader,nounits")
        .output()
    else {
        return Vec::new();
    };

    if !out.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split(',').map(|f| f.trim().to_string()).collect())
        .collect()
}

fn gpu_specs() -> Vec<Gpu> {
    nvidia_smi("name,memory.total")
        .into_iter()
        .map(|row| Gpu {
            name: row.first().cloned().unwrap_or_else(|| "gpu".to_string()),
            vram_mb: row.get(1).and_then(|v| v.parse().ok()),
        })
        .collect()
}

fn gpu_health() -> Vec<GpuHealth> {
    nvidia_smi("name,memory.free,memory.total,utilization.gpu,temperature.gpu")
        .into_iter()
        .map(|row| GpuHealth {
            name: row.first().cloned().unwrap_or_else(|| "gpu".to_string()),
            vram_free_mb: row.get(1).and_then(|v| v.parse().ok()),
            vram_total_mb: row.get(2).and_then(|v| v.parse().ok()),
            utilization_percent: row.get(3).and_then(|v| v.parse().ok()),
            temperature_c: row.get(4).and_then(|v| v.parse().ok()),
        })
        .collect()
}
