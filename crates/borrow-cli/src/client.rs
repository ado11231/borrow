//! Talking to the Agent's daemon from the Client.
//!
//! One connection carries one request and one response, both as a single line of
//! JSON. Small and boring on purpose: this is the control plane, and the real work
//! travels over ssh instead.

use borrow_core::protocol::{Request, Response};
use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Send one request to a daemon and wait for its answer. An `Error` response is
/// turned into a real error here, so callers only ever see the happy shapes.
pub async fn request(host: &str, port: u16, message: Request) -> anyhow::Result<Response> {
    let mut stream = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("could not reach the borrow daemon at {host}:{port}"))?;

    let mut line = serde_json::to_string(&message).context("could not encode the request")?;
    line.push('\n');

    stream.write_all(line.as_bytes()).await.context("could not send the request")?;

    let mut reply = String::new();
    BufReader::new(&mut stream)
        .read_line(&mut reply)
        .await
        .context("the daemon closed the connection without answering")?;

    let response: Response =
        serde_json::from_str(reply.trim()).context("could not understand the daemon's answer")?;

    match response {
        Response::Error { message } => anyhow::bail!("{message}"),
        other => Ok(other),
    }
}
