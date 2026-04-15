//! TestForge CLI — semantic code search and AI-powered test generation.

mod commands;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "testforge",
    version,
    about = "Semantic code search and AI-powered test generation",
    long_about = "TestForge indexes your codebase semantically, lets you search it in natural \
                  language, and generates contextually-aware tests using AI."
)]
struct Cli {
    #[command(subcommand)]
    command: commands::Command,

    /// Enable verbose logging (repeat for more: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Configure logging based on verbosity
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    commands::execute(cli.command).await
}
