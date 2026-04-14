//! `testforge status` — show the current index status.

use clap::Args;
use colored::Colorize;
use testforge_core::Config;
use testforge_indexer::Indexer;

#[derive(Args)]
pub struct StatusArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

pub fn run(args: StatusArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (config, project_root) = Config::discover(&cwd)?;
    let indexer = Indexer::new(config, &project_root)?;

    let status = indexer.status()?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!();
    println!(
        "  {} TestForge Index — {}",
        "◆".cyan().bold(),
        project_root.display()
    );
    println!();
    println!(
        "  Files indexed:    {}",
        status.file_count.to_string().cyan()
    );
    println!(
        "  Symbols extracted: {}",
        status.symbol_count.to_string().cyan()
    );
    println!(
        "  Embeddings:        {}",
        if status.embedding_count > 0 {
            status.embedding_count.to_string().green()
        } else {
            "0 (not computed yet)".yellow()
        }
    );

    if !status.languages.is_empty() {
        let langs: Vec<_> = status.languages.iter().map(|l| l.to_string()).collect();
        println!("  Languages:         {}", langs.join(", ").cyan());
    }

    if let Some(ts) = status.last_indexed {
        let ago = chrono::Utc::now() - ts;
        let human = if ago.num_seconds() < 60 {
            format!("{}s ago", ago.num_seconds())
        } else if ago.num_minutes() < 60 {
            format!("{}m ago", ago.num_minutes())
        } else if ago.num_hours() < 24 {
            format!("{}h ago", ago.num_hours())
        } else {
            format!("{}d ago", ago.num_days())
        };
        println!("  Last indexed:      {}", human.dimmed());
    } else {
        println!(
            "  Last indexed:      {}",
            "never — run `testforge index .`".yellow()
        );
    }

    println!(
        "  Watcher:           {}",
        if status.watcher_active {
            "active".green()
        } else {
            "inactive".dimmed()
        }
    );
    println!();

    Ok(())
}
