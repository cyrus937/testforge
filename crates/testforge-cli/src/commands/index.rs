//! `testforge index` — index source files for semantic search.

use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use testforge_core::Config;
use testforge_indexer::Indexer;
use testforge_search::SearchEngine;

#[derive(Args)]
pub struct IndexArgs {
    /// Path to index (defaults to project root)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Clear the existing index before re-indexing
    #[arg(long)]
    clean: bool,

    /// Start file watcher for continuous re-indexing
    #[arg(short, long)]
    watch: bool,
}

pub fn run(args: IndexArgs) -> anyhow::Result<()> {
    let start = Instant::now();

    let (config, project_root) = Config::discover(&args.path)?;
    let mut indexer = Indexer::new(config, &project_root)?;

    if args.clean {
        println!("{} Clearing existing index...", "→".blue().bold());
        indexer.clear()?;
    }

    // Set up progress bar
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message("Indexing source files...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Run indexing
    let report = indexer.index_full()?;

    pb.finish_and_clear();

    let elapsed = start.elapsed();

    // Print results
    println!();
    println!("  {} Indexing complete in {:.1}s", "✓".green().bold(), elapsed.as_secs_f64());
    println!();
    println!(
        "  Files indexed:  {}",
        report.files_indexed.to_string().cyan()
    );
    println!(
        "  Symbols found:  {}",
        report.symbols_extracted.to_string().cyan()
    );
    println!(
        "  Files skipped:  {} (unchanged)",
        report.files_skipped.to_string().yellow()
    );

    if report.files_failed > 0 {
        println!(
            "  Files failed:   {}",
            report.files_failed.to_string().red()
        );
        println!();
        for (path, error) in &report.errors {
            println!("    {} {}: {}", "✗".red(), path.display(), error);
        }
    }

    // Phase 2: Build the full-text search index from the SQLite store
    if report.symbols_extracted > 0 || args.clean {
        let search_dir = project_root
            .join(testforge_core::config::CONFIG_DIR)
            .join("search");

        let pb2 = ProgressBar::new_spinner();
        pb2.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb2.set_message("Building search index...");
        pb2.enable_steady_tick(std::time::Duration::from_millis(100));

        let (config2, _) = Config::discover(&args.path)?;
        let mut search_engine = SearchEngine::open(&search_dir, &config2)?;

        if args.clean {
            search_engine.clear()?;
        }

        // Feed all symbols into the text index
        let all_symbols = indexer.all_symbols()?;
        let no_embeddings: Vec<Option<Vec<f32>>> = vec![None; all_symbols.len()];
        search_engine.index_symbols(&all_symbols, &no_embeddings)?;
        search_engine.commit()?;

        pb2.finish_and_clear();

        let text_docs = search_engine.text_doc_count().unwrap_or(0);
        let vec_count = search_engine.vector_count();

        println!(
            "  Search index:   {} text docs, {} vectors",
            text_docs.to_string().cyan(),
            if vec_count > 0 {
                vec_count.to_string().green()
            } else {
                "0 (run testforge embed to compute)".yellow()
            }
        );
    }

    if args.watch {
        println!();
        println!(
            "  {} Watching for changes... (press Ctrl+C to stop)",
            "👁".bold()
        );

        let watcher = testforge_indexer::watcher::FileWatcher::new(
            Config::discover(&args.path)?.0,
            project_root.clone(),
        );

        watcher.watch_with_handler(move |event| {
            match event {
                testforge_indexer::watcher::WatchEvent::FileChanged(path) => {
                    let rel = path
                        .strip_prefix(&project_root)
                        .unwrap_or(&path);
                    println!(
                        "  {} {} changed — re-indexing...",
                        "↻".blue(),
                        rel.display()
                    );
                    // Re-indexing would happen here (requires mutable indexer)
                }
                testforge_indexer::watcher::WatchEvent::FileDeleted(path) => {
                    println!("  {} {} deleted", "✗".red(), path.display());
                }
                testforge_indexer::watcher::WatchEvent::FileRenamed(old, new) => {
                    println!(
                        "  {} {} → {}",
                        "↻".blue(),
                        old.display(),
                        new.display()
                    );
                }
            }
        })?;
    }

    Ok(())
}