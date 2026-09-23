//! `slingshot top`: live Agent resources with the active Slingshot jobs underneath.

use crate::client::{Control, unexpected};
use crate::live;
use slingshot_core::config::Config;
use slingshot_core::control::{Request, Response};
use slingshot_core::presentation::Style;
use slingshot_core::storage;

pub async fn top(agent: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    live::require_terminal()?;
    let control = tokio::sync::Mutex::new(Control::connect(target).await?);
    let name = target.name.clone();
    live::show(|| async {
        let mut control = control.lock().await;
        let health = super::health::fetch(&mut control).await?;
        let Response::Jobs(jobs) = control.call(Request::Jobs { all: false }).await? else {
            return Err(unexpected());
        };
        let style = Style::stdout();
        let mut body = super::health::render(&name, &health, style);
        body.push_str(&super::ps::render(
            &name,
            &jobs,
            false,
            storage::now(),
            style,
        ));
        Ok(body)
    })
    .await
}
