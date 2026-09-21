//! Agent resource use: one snapshot, or a live view with `--watch`.

use crate::client::{self, Control, unexpected};
use crate::live;
use borrow_core::config::Config;
use borrow_core::control::{Request, Response};
use borrow_core::presentation::{Style, Tone, capacity, row};
use borrow_core::protocol::Health;

pub async fn health(agent: Option<String>, watch: bool) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    if !watch {
        let Response::Health(health) = client::request(target, Request::Health).await? else {
            return Err(unexpected());
        };
        print!("{}", render(&target.name, &health, Style::stdout()));
        return Ok(0);
    }
    live::require_terminal()?;
    let control = tokio::sync::Mutex::new(Control::connect(target).await?);
    let name = target.name.clone();
    live::show(|| async {
        let health = fetch(&mut *control.lock().await).await?;
        Ok(render(&name, &health, Style::stdout()))
    })
    .await
}

pub async fn fetch(control: &mut Control) -> anyhow::Result<Health> {
    match control.call(Request::Health).await? {
        Response::Health(health) => Ok(health),
        _ => Err(unexpected()),
    }
}

pub fn render(name: &str, health: &Health, style: Style) -> String {
    let mut output = format!("\n{}\n\n", style.heading(format!("Agent: {name}")));
    output.push_str(&row("CPU", load(health.cpu_percent as f64, style)));
    output.push_str(&row(
        "RAM",
        memory(health.memory_used_mb, health.memory_total_mb, style),
    ));
    output.push_str(&row(
        "Disk",
        format!("{} free", capacity(health.disk_free_mb)),
    ));
    if let Some(free) = health.workspace_free_mb {
        output.push_str(&row("Workspace", workspace(free, style)));
    }

    match (&health.gpu_problem, health.gpus.is_empty()) {
        (Some(problem), _) => output.push_str(&row("GPU", style.paint(problem, Tone::Warning))),
        (None, true) => output.push_str(&row("GPU", "No GPU data available")),
        (None, false) => {}
    }
    for (index, gpu) in health.gpus.iter().enumerate() {
        output.push('\n');
        output.push_str(&row(
            &format!("GPU {}", index + 1),
            style.heading(&gpu.name),
        ));
        let usage = gpu
            .utilization_percent
            .map(|value| load(value as f64, style))
            .unwrap_or_else(|| "Unavailable".to_string());
        output.push_str(&row("Usage", usage));
        let vram = match (gpu.vram_free_mb, gpu.vram_total_mb) {
            (Some(free), Some(total)) if free <= total => memory(total - free, total, style),
            _ => "Unavailable".to_string(),
        };
        output.push_str(&row("VRAM", vram));
        let temperature = gpu
            .temperature_c
            .map(|value| {
                let (label, tone) = rating(value as f64, 75.0, 85.0, ["Normal", "Warm", "Hot"]);
                style.paint(format!("{value}°C  {label}"), tone)
            })
            .unwrap_or_else(|| "Unavailable".to_string());
        output.push_str(&row("Temperature", temperature));
    }
    if health.swap_total_mb == 0 {
        output.push('\n');
        output.push_str(&style.status(
            "No swap configured. Jobs may stop if RAM runs out.",
            Tone::Warning,
        ));
        output.push('\n');
    }
    output.push('\n');
    output
}

fn workspace(free_mb: u64, style: Style) -> String {
    match free_mb < borrow_core::telemetry::DISK_WARNING_MB {
        true => style.paint(
            format!("{} free  Low space", capacity(free_mb)),
            Tone::Error,
        ),
        false => format!("{} free", capacity(free_mb)),
    }
}

fn rating(value: f64, warning: f64, high: f64, labels: [&str; 3]) -> (&str, Tone) {
    if value >= high {
        (labels[2], Tone::Error)
    } else if value >= warning {
        (labels[1], Tone::Warning)
    } else {
        (labels[0], Tone::Good)
    }
}

fn load(percent: f64, style: Style) -> String {
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return "Unavailable".to_string();
    }
    let (label, tone) = rating(percent, 70.0, 90.0, ["Light", "Busy", "High load"]);
    style.paint(format!("{percent:.1}%  {label}"), tone)
}

fn memory(used: u64, total: u64, style: Style) -> String {
    if total == 0 || used > total {
        return "Unavailable".to_string();
    }
    let percent = used as f64 / total as f64 * 100.0;
    let (label, tone) = rating(
        percent,
        75.0,
        90.0,
        ["Available", "Limited", "Low free memory"],
    );
    style.paint(
        format!(
            "{} / {} used, {} free  {label}",
            capacity(used),
            capacity(total),
            capacity(total - used)
        ),
        tone,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrow_core::protocol::GpuHealth;

    fn sample() -> Health {
        Health {
            cpu_percent: 42.0,
            memory_used_mb: 18432,
            memory_total_mb: 65536,
            swap_total_mb: 4096,
            disk_free_mb: 419840,
            workspace_free_mb: Some(1024),
            gpus: vec![GpuHealth {
                name: "Example GPU".to_string(),
                vram_free_mb: Some(14336),
                vram_total_mb: Some(24576),
                utilization_percent: Some(71),
                temperature_c: Some(76),
            }],
            gpu_problem: None,
        }
    }

    #[test]
    fn usage_thresholds_include_the_boundary() {
        for (value, status, code) in [
            (69.0, "Light", 32),
            (70.0, "Busy", 33),
            (89.0, "Busy", 33),
            (90.0, "High load", 31),
        ] {
            let text = load(value, Style::new(true));
            assert!(text.contains(status));
            assert!(text.starts_with(&format!("\x1b[{code}m")));
        }
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            assert_eq!(load(invalid, Style::new(true)), "Unavailable");
        }
    }

    #[test]
    fn memory_thresholds_and_invalid_totals() {
        for (used, status) in [
            (74, "Available"),
            (75, "Limited"),
            (89, "Limited"),
            (90, "Low free memory"),
        ] {
            assert!(memory(used, 100, Style::new(false)).ends_with(status));
        }
        assert_eq!(memory(0, 0, Style::new(true)), "Unavailable");
        assert_eq!(memory(101, 100, Style::new(true)), "Unavailable");
        assert!(memory(1024, 1536, Style::new(false)).contains("512.0 MiB free"));
    }

    #[test]
    fn temperature_thresholds() {
        let mut health = sample();
        for (temperature, label, code) in [
            (74, "Normal", 32),
            (75, "Warm", 33),
            (84, "Warm", 33),
            (85, "Hot", 31),
        ] {
            health.gpus[0].temperature_c = Some(temperature);
            let output = render("archbox", &health, Style::new(true));
            assert!(output.contains(&format!("\x1b[{code}m{temperature}°C  {label}")));
        }
    }

    #[test]
    fn missing_and_multiple_gpu_measurements() {
        let mut health = sample();
        health.gpus.push(GpuHealth {
            name: "Second GPU".to_string(),
            vram_free_mb: None,
            vram_total_mb: None,
            utilization_percent: None,
            temperature_c: None,
        });
        let output = render("archbox", &health, Style::new(false));
        assert!(output.contains("GPU 1"));
        assert!(output.contains("GPU 2"));
        assert_eq!(output.matches("Unavailable").count(), 3);
        health.gpus[0].vram_free_mb = Some(999999);
        assert!(render("archbox", &health, Style::new(false)).contains("VRAM         Unavailable"));
        health.gpus.clear();
        assert!(render("archbox", &health, Style::new(false)).contains("No GPU data available"));
        health.gpu_problem = Some("Reboot the Agent to fix".to_string());
        let output = render("archbox", &health, Style::new(false));
        assert!(
            output.contains("GPU          Reboot the Agent to fix"),
            "{output}"
        );
        assert!(!output.contains("No GPU data available"));
    }

    #[test]
    fn plain_snapshot_has_readable_units_and_no_escapes() {
        let output = render("archbox", &sample(), Style::new(false));
        assert!(output.contains("CPU          42.0%  Light"));
        assert!(output.contains("18.0 GiB / 64.0 GiB used, 46.0 GiB free  Available"));
        assert!(output.contains("410.0 GiB free"));
        assert!(output.contains("Workspace    1.0 GiB free  Low space"));
        assert!(output.contains("71.0%  Busy"));
        assert!(output.contains("76°C  Warm"));
        assert!(!output.contains('\x1b'));
        assert!(!output.contains("No swap"));
    }

    #[test]
    fn swap_notice_is_a_readable_warning() {
        let mut health = sample();
        health.swap_total_mb = 0;
        assert!(
            render("archbox", &health, Style::new(false)).contains("Warning: No swap configured.")
        );
    }
}
