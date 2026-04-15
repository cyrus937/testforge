//! CLI command definitions and dispatcher.

pub mod gen_tests;
pub mod index;
pub mod init;
pub mod search;
pub mod serve;
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

    /// Generate tests for a function, method, or file
    GenTests(gen_tests::GenTestsArgs),

    /// Start the API server
    Serve(serve::ServeArgs),

    /// Show the current index status
    Status(status::StatusArgs),
}

/// Execute the given CLI command.
pub async fn execute(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Init(args) => init::run(args),
        Command::Index(args) => index::run(args),
        Command::Search(args) => search::run(args),
        Command::GenTests(args) => gen_tests::run(args),
        Command::Serve(args) => serve::run(args).await,
        Command::Status(args) => status::run(args),
    }
}
