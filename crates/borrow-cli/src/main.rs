//! borrow: run heavy work on another machine, from a light one.

mod client;
mod commands;
mod keys;
mod ssh;

use borrow_core::protocol::DEFAULT_PORT;
use clap::{Parser, Subcommand};

/// The command line, described as a type. clap derives the parser and `--help` from it.
///
/// `--agent` lives up here rather than on `run`, because `run` swallows everything
/// after it so the remote command can have flags of its own.
#[derive(Parser)]
#[command(version, about = "Run heavy work on another machine")]
struct Cli {
    /// Which box to use. Defaults to the only one, or the one marked default.
    #[arg(long, short, global = true)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

/// One variant per subcommand; its fields are that subcommand's arguments.
#[derive(Subcommand)]
enum Commands {
    #[command(about = "Agent: start the daemon and print a pairing code")]
    Serve {
        /// The name this box will be known by. Defaults to its hostname.
        #[arg(long)]
        name: Option<String>,

        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    #[command(about = "Client: pair with a box using the code borrow serve printed")]
    Link {
        code: String,

        /// Save the box under a different name than the one it calls itself.
        #[arg(long)]
        name: Option<String>,
    },

    #[command(about = "Remove borrow's key from a box and forget it")]
    Unlink,

    /// `trailing_var_arg` stops clap parsing after `run`, so flags like `--release`
    /// reach the remote program untouched rather than being claimed by borrow.
    #[command(about = "Run a command on the Agent and stream its output back")]
    Run {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },

    #[command(about = "Static specs of the box, from the cache")]
    Info {
        /// Ask the box again instead of using what was saved at pairing.
        #[arg(long)]
        refresh: bool,
    },

    #[command(about = "Live snapshot: cpu, memory, gpu, disk")]
    Health,
}

/// Parse and dispatch. `Ok(code)` is the remote command's exit status and is passed
/// through; `Err` means borrow itself failed.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("BORROW_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Serve { name, port } => borrow_agent::serve(name, port).await,
        Commands::Link { code, name } => commands::link::link(code, name).await,
        Commands::Unlink => commands::unlink::unlink(cli.agent).await,
        Commands::Run { cmd } => commands::run::run(cli.agent, cmd).await,
        Commands::Info { refresh } => commands::info::info(cli.agent, refresh).await,
        Commands::Health => commands::health::health(cli.agent).await,
    };

    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("borrow: {e:#}");
            std::process::exit(1);
        }
    }
}
