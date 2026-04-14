//! `testforge init` — initialize a new TestForge project.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use testforge_core::Config;

#[derive(Args)]
pub struct InitArgs {
    /// Project root directory (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Project name (defaults to directory name)
    #[arg(short, long)]
    name: Option<String>,

    /// Languages to index (comma-separated, e.g. "python,rust")
    #[arg(short, long, value_delimiter = ',')]
    languages: Option<Vec<String>>,
}

pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let root = std::fs::canonicalize(&args.path)?;

    println!(
        "{} Initializing TestForge in {}",
        "→".blue().bold(),
        root.display()
    );

    let config_path = Config::init(&root, args.name.as_deref())?;

    // If languages were specified, update the config
    if let Some(languages) = args.languages {
        let mut config = Config::load(&config_path)?;
        config.project.languages = languages;
        config.save(&config_path)?;
    }

    println!();
    println!(
        "  {} Created {}",
        "✓".green().bold(),
        ".testforge/config.toml"
    );
    println!("  {} Created {}", "✓".green().bold(), ".testforge/index/");
    println!("  {} Created {}", "✓".green().bold(), ".testforge/cache/");
    println!();
    println!(
        "  {} Run {} to build the search index.",
        "Next:".bold(),
        "testforge index .".cyan()
    );

    Ok(())
}
