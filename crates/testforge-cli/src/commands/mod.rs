//! CLI command definitions and dispatcher.

pub mod index;
pub mod init;
pub mod search;
pub mod status;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Command {
    /// Initialize TestForge in the current project
    Init(init::InitArgs),

    /// Index source files for semantic search
    Index(index::IndexArgs),

    /// Search the codebase using natural language
    Search(search::SearchArgs),

    /// Show the current index status
    Status(status::StatusArgs),
}

/// Execute the given CLI command.
pub fn execute(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Init(args) => init::run(args),
        Command::Index(args) => index::run(args),
        Command::Search(args) => search::run(args),
        Command::Status(args) => status::run(args),
    }
}
