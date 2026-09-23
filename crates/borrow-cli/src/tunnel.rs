//! The Client end of reaching an Agent over iroh. ssh starts `borrow internal-tunnel` as
//! its ProxyCommand and speaks to the Agent's sshd through this process's standard input
//! and output, so ssh itself is unchanged whichever path carries it.

use crate::project;
use anyhow::Context;
use borrow_core::tunnel::{self, ALPN};
use iroh::Endpoint;
use iroh::endpoint::{Connection, presets};
use tokio::io::{AsyncRead, AsyncWrite};

/// The ProxyCommand line for reaching the Agent with iroh key `key`, which the route has
/// already checked. ssh runs it through a shell after expanding its own `%` tokens, so
/// every part is quoted and every `%` doubled.
pub fn proxy_command(key: &str) -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| "borrow".to_string());
    proxy_line(&exe, key)
}

fn proxy_line(exe: &str, key: &str) -> String {
    shell_words::join([exe, "internal-tunnel", key]).replace('%', "%%")
}

/// Connect to the Agent and carry ssh's bytes until either side closes.
pub async fn run(key: String) -> anyhow::Result<i32> {
    let agent = tunnel::public_key(&key)?;
    let identity = tunnel::identity(&project::client_root()?)?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(identity)
        .bind()
        .await
        .context("Could not start iroh")?;
    let connection = endpoint.connect(agent, ALPN).await.context(
        "Could not reach the Agent over iroh. It may be off, asleep, or not running borrow serve",
    )?;
    let result = pipe(&connection, tokio::io::stdin(), tokio::io::stdout()).await;
    connection.close(0u32.into(), b"done");
    endpoint.close().await;
    result.map(|_| 0)
}

async fn pipe(
    connection: &Connection,
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin,
) -> anyhow::Result<()> {
    let (send, recv) = connection
        .open_bi()
        .await
        .context("Could not open a stream to the Agent")?;
    let mut remote = tokio::io::join(recv, send);
    let mut local = tokio::io::join(input, output);
    tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::RelayMode;
    use iroh::SecretKey;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn the_proxy_command_names_the_key() {
        let key = SecretKey::generate().public().to_string();
        let command = proxy_command(&key);

        assert!(
            command.ends_with(&format!(" internal-tunnel {key}")),
            "{command}"
        );
    }

    #[test]
    fn nothing_in_the_command_can_become_shell_syntax_or_an_ssh_token() {
        assert_eq!(
            proxy_line("/App 100%/borrow", "k"),
            "'/App 100%%/borrow' internal-tunnel k"
        );
    }

    async fn endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .alpns(alpns)
            .bind()
            .await
            .unwrap()
    }

    /// Stands in for the Agent: echoes every stream back.
    async fn echo_agent() -> Endpoint {
        let agent = endpoint(vec![ALPN.to_vec()]).await;
        let accepting = agent.clone();
        tokio::spawn(async move {
            while let Some(incoming) = accepting.accept().await {
                let connection = incoming.await.unwrap();
                while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                    tokio::io::copy(&mut recv, &mut send).await.unwrap();
                    send.finish().unwrap();
                }
            }
        });
        agent
    }

    #[tokio::test]
    async fn bytes_cross_in_both_directions() {
        let agent = echo_agent().await;
        let client = endpoint(Vec::new()).await;
        let connection = client.connect(agent.addr(), ALPN).await.unwrap();
        let (mut ssh_in, input) = tokio::io::duplex(64);
        let (output, mut ssh_out) = tokio::io::duplex(64);

        ssh_in.write_all(b"SSH-2.0-test\r\n").await.unwrap();
        drop(ssh_in);
        pipe(&connection, input, output).await.unwrap();
        let mut echoed = Vec::new();
        ssh_out.read_to_end(&mut echoed).await.unwrap();

        assert_eq!(echoed, b"SSH-2.0-test\r\n");
    }
}
