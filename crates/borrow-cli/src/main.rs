//! borrow: run heavy work on another machine, from a light one.

mod client;
mod commands;
mod keys;
mod ssh;

use borrow_core::presentation::{self, ColorMode, Style, Tone};
use borrow_core::protocol::DEFAULT_PORT;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

/// Global options precede run so the remote command can receive its own flags.
#[derive(Parser)]
#[command(name = "borrow", version, about = "Run heavy work on another machine", long_about = None)]
struct Cli {
    /// Control terminal colors.
    #[arg(long, global = true, value_parser = ["auto", "always", "never"], default_value = "auto")]
    color: String,

    /// Which box to use. Defaults to the only one, or the one marked default.
    #[arg(long, short, global = true)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

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

    #[command(about = "Current CPU, RAM, GPU, and disk usage")]
    Health,
}

/// Pass through command exit codes. Borrow failures exit with code 1.
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let mode = color_mode(&args);
    presentation::configure(mode);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("BORROW_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(mode.enabled(
            std::io::IsTerminal::is_terminal(&std::io::stderr()),
            presentation::no_color(),
            presentation::dumb_terminal(),
        ))
        .init();

    let color = match mode {
        ColorMode::Always => clap::ColorChoice::Always,
        ColorMode::Never => clap::ColorChoice::Never,
        ColorMode::Auto if presentation::no_color() || presentation::dumb_terminal() => {
            clap::ColorChoice::Never
        }
        ColorMode::Auto => clap::ColorChoice::Auto,
    };
    let matches = Cli::command().color(color).get_matches_from(args);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());

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
            eprintln!("{}", Style::stderr().status(format!("{e:#}"), Tone::Error));
            std::process::exit(1);
        }
    }
}

/// Read color before Clap renders help. Stop at run so remote flags remain untouched.
fn color_mode(args: &[std::ffi::OsString]) -> ColorMode {
    let mut mode = ColorMode::Auto;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy();
        if arg == "run" || arg == "--" {
            break;
        }
        if matches!(arg.as_ref(), "--agent" | "-a" | "--name" | "--port") {
            args.next();
            continue;
        }
        let value = if arg == "--color" {
            args.next().map(|value| value.to_string_lossy())
        } else {
            arg.strip_prefix("--color=").map(std::borrow::Cow::Borrowed)
        };
        mode = match value.as_deref() {
            Some("always") => ColorMode::Always,
            Some("never") => ColorMode::Never,
            Some("auto") => ColorMode::Auto,
            _ => mode,
        };
    }
    mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_flags_stay_with_the_remote_command() {
        let cli = Cli::try_parse_from([
            "borrow",
            "--color",
            "never",
            "run",
            "cargo",
            "--color",
            "always",
            "--release",
        ])
        .unwrap();
        let Commands::Run { cmd } = cli.command else {
            panic!("Expected run")
        };
        assert_eq!(cmd, ["cargo", "--color", "always", "--release"]);
        assert_eq!(cli.color, "never");
    }

    #[test]
    fn color_scan_respects_option_values_and_run_boundary() {
        for (args, expected) in [
            (
                vec!["borrow", "--agent", "run", "--color=always", "info"],
                ColorMode::Always,
            ),
            (
                vec!["borrow", "run", "echo", "--color=always"],
                ColorMode::Auto,
            ),
            (
                vec!["borrow", "--color", "never", "--help"],
                ColorMode::Never,
            ),
        ] {
            let args: Vec<_> = args.into_iter().map(std::ffi::OsString::from).collect();
            assert_eq!(color_mode(&args), expected);
        }
    }
}
