//! borrow — run heavy work on another machine, from a light one.

mod ssh;
mod commands;

use clap::{Parser, Subcommand};

/// The command line, described as a type. clap derives the parser and `--help` from it.
#[derive(Parser)]
#[command(version, about = "Run heavy work on another machine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// One variant per subcommand; its fields are that subcommand's arguments.
/// `trailing_var_arg` stops clap parsing at `run`, so flags like `--release` reach the
/// remote program untouched rather than being claimed by borrow.
#[derive(Subcommand)]
enum Commands {
    #[command(about = "Run a command on the Agent and stream its output back")]
    Run {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
}

/// Parse and dispatch. `Ok(code)` is the remote command's exit status and is passed
/// through; `Err` means borrow itself failed.
#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Run { cmd } => commands::run::run(cmd).await,
    };

    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("borrow: {e:#}");
            std::process::exit(1);
        }
    }
}
