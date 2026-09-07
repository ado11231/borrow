//! `borrow info`: what the box is.

use crate::client;
use crate::config::Config;
use crate::protocol::{Request, Response, Specs};

/// Print the box's static specs. These come from the cache saved at pairing, so the
/// usual case is instant and works even when the box is asleep.
pub async fn info(agent: Option<String>, refresh: bool) -> anyhow::Result<i32> {
    let mut config = Config::load()?;
    let target = config.resolve(agent.as_deref())?.clone();

    let specs = match (refresh, &target.specs) {
        (false, Some(cached)) => cached.clone(),
        _ => {
            let response =
                client::request(&target.host, target.daemon_port(), Request::Info).await?;

            let Response::Info(specs) = response else {
                anyhow::bail!("the box answered something unexpected");
            };

            let mut updated = target.clone();
            updated.specs = Some(specs.clone());
            config.upsert(updated);
            config.save()?;

            specs
        }
    };

    print(&target.name, &target.host, &specs);

    Ok(0)
}

fn print(name: &str, host: &str, specs: &Specs) {
    println!();
    println!("{name}  ({host})");
    println!();
    println!("  os      {}", specs.os);
    println!("  kernel  {}", specs.kernel);
    println!("  cpu     {} ({} cores)", specs.cpu, specs.cores);
    println!("  memory  {} GB", specs.memory_mb / 1024);
    println!("  disk    {} GB", specs.disk_total_mb / 1024);

    for gpu in &specs.gpus {
        match gpu.vram_mb {
            Some(vram) => println!("  gpu     {} ({} GB vram)", gpu.name, vram / 1024),
            None => println!("  gpu     {}", gpu.name),
        }
    }

    if !specs.tools.is_empty() {
        println!("  tools   {}", specs.tools.join(", "));
    }

    println!();
}
