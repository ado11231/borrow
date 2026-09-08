//! `borrow health`: what the box is doing right now.

use crate::client;
use borrow_core::config::Config;
use borrow_core::protocol::{Health, Request, Response};

/// Ask the box for a live snapshot. Always fetched, never cached, because a cached
/// answer to "is there room for this build" is worse than no answer at all.
pub async fn health(agent: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;

    let response = client::request(&target.host, target.daemon_port(), Request::Health).await?;

    let Response::Health(health) = response else {
        anyhow::bail!("the box answered something unexpected");
    };

    print(&target.name, &health);

    Ok(0)
}

fn print(name: &str, health: &Health) {
    let free_memory = health.memory_total_mb.saturating_sub(health.memory_used_mb);

    println!();
    println!("{name}");
    println!();
    println!("  cpu     {:.0}%", health.cpu_percent);
    println!(
        "  memory  {} / {} GB used, {} GB free",
        health.memory_used_mb / 1024,
        health.memory_total_mb / 1024,
        free_memory / 1024
    );
    println!("  disk    {} GB free", health.disk_free_mb / 1024);

    for gpu in &health.gpus {
        let vram = match (gpu.vram_free_mb, gpu.vram_total_mb) {
            (Some(free), Some(total)) => format!("{free} / {total} MB free"),
            _ => "vram unknown".to_string(),
        };

        let load = gpu.utilization_percent.map(|u| format!("{u}%")).unwrap_or_default();
        let temperature = gpu.temperature_c.map(|t| format!("{t}°C")).unwrap_or_default();

        println!("  gpu     {} {vram} {load} {temperature}", gpu.name);
    }

    if health.swap_total_mb == 0 {
        println!();
        println!("  note: no swap, so a build that runs out of memory gets killed outright");
    }

    println!();
}
