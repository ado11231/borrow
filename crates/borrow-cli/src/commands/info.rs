//! `borrow info`: what the box is.

use crate::client::{self, unexpected};
use borrow_core::config::Config;
use borrow_core::control::{Request, Response};
use borrow_core::presentation::{Style, capacity, row};
use borrow_core::protocol::Specs;

/// Use cached specifications unless a refresh is requested.
pub async fn info(agent: Option<String>, refresh: bool) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();

    let specs = match (refresh, &target.specs) {
        (false, Some(cached)) => cached.clone(),
        _ => {
            let Response::Info(specs) = client::request(&target, Request::Info).await? else {
                return Err(unexpected());
            };

            let mut updated = target.clone();
            updated.specs = Some(specs.clone());
            config.upsert(updated);
            config.save()?;

            specs
        }
    };

    print!(
        "{}",
        render(&target.name, &target.host, &specs, Style::stdout())
    );

    Ok(0)
}

fn render(name: &str, host: &str, specs: &Specs, style: Style) -> String {
    let mut output = format!("\n{}\n\n", style.heading(format!("Agent: {name} ({host})")));
    output.push_str(&row("OS", &specs.os));
    output.push_str(&row("Kernel", &specs.kernel));
    output.push_str(&row(
        "CPU",
        format!("{} ({} cores)", specs.cpu, specs.cores),
    ));
    output.push_str(&row("RAM", capacity(specs.memory_mib)));
    output.push_str(&row("Disk", capacity(specs.disk_total_mib)));
    if specs.gpus.is_empty() {
        output.push_str(&row("GPU", "No GPU data available"));
    }
    for (index, gpu) in specs.gpus.iter().enumerate() {
        output.push('\n');
        output.push_str(&row(&format!("GPU {}", index + 1), &gpu.name));
        output.push_str(&row(
            "VRAM",
            gpu.vram_mib
                .map(capacity)
                .unwrap_or_else(|| "Unavailable".to_string()),
        ));
    }
    output.push('\n');
    output.push_str(&row(
        "Tools",
        if specs.tools.is_empty() {
            "None detected".to_string()
        } else {
            specs.tools.join(", ")
        },
    ));
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrow_core::protocol::Gpu;

    #[test]
    fn specs_preserve_names_and_show_small_capacities() {
        let mut specs = Specs {
            name: "archbox".into(),
            os: "Linux".into(),
            kernel: "Example".into(),
            cpu: "Example CPU".into(),
            cores: 8,
            memory_mib: 1536,
            disk_total_mib: 10240,
            tools: vec!["tmux".into()],
            gpus: vec![Gpu {
                name: "Example GPU".into(),
                vram_mib: Some(512),
            }],
        };
        let text = render("archbox", "archbox.local", &specs, Style::new(false));
        assert!(text.contains("Agent: archbox (archbox.local)"));
        assert!(text.contains("1.5 GiB"));
        assert!(text.contains("512.0 MiB"));
        assert!(text.contains("Tools        tmux"));
        assert!(!text.contains('\x1b'));
        specs.gpus[0].vram_mib = None;
        assert!(render("archbox", "host", &specs, Style::new(false)).contains("Unavailable"));
        specs.gpus.clear();
        specs.tools.clear();
        let text = render("archbox", "host", &specs, Style::new(false));
        assert!(text.contains("No GPU data available"));
        assert!(text.contains("None detected"));
    }
}
