//! `testforge search` — search the codebase using natural language or keywords.

use clap::Args;
use colored::Colorize;
use testforge_core::Config;
use testforge_indexer::Indexer;

#[derive(Args)]
pub struct SearchArgs {
    /// Search query (natural language or keywords)
    query: String,

    /// Maximum number of results
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Filter by language (e.g., "python", "rust")
    #[arg(short = 'L', long)]
    language: Option<String>,

    /// Filter by symbol kind (function, class, method, struct, etc.)
    #[arg(short, long)]
    kind: Option<String>,

    /// Output format: "pretty" (default) or "json"
    #[arg(short, long, default_value = "pretty")]
    format: String,
}

pub fn run(args: SearchArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (config, project_root) = Config::discover(&cwd)?;
    let indexer = Indexer::new(config, &project_root)?;

    // Phase 1: keyword-based search against the SQLite index.
    // Phase 2+ will add semantic (vector) search via the Python bridge.
    let results = indexer.all_symbols()?;

    let query_lower = args.query.to_lowercase();

    let mut filtered: Vec<_> = results
        .into_iter()
        .filter(|sym| {
            // Text matching across name, qualified_name, docstring, source
            let name_match = sym.name.to_lowercase().contains(&query_lower)
                || sym.qualified_name.to_lowercase().contains(&query_lower);
            let doc_match = sym
                .docstring
                .as_ref()
                .map(|d| d.to_lowercase().contains(&query_lower))
                .unwrap_or(false);
            let source_match = sym.source.to_lowercase().contains(&query_lower);

            name_match || doc_match || source_match
        })
        .filter(|sym| {
            // Optional language filter
            if let Some(ref lang) = args.language {
                sym.language.to_string() == lang.to_lowercase()
            } else {
                true
            }
        })
        .filter(|sym| {
            // Optional kind filter
            if let Some(ref kind) = args.kind {
                sym.kind.to_string() == kind.to_lowercase()
            } else {
                true
            }
        })
        .collect();

    // Score: prefer name matches over source matches
    filtered.sort_by(|a, b| {
        let score_a = search_score(a, &query_lower);
        let score_b = search_score(b, &query_lower);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    filtered.truncate(args.limit);

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    // Pretty print
    if filtered.is_empty() {
        println!(
            "  {} No results for \"{}\"",
            "○".yellow(),
            args.query.bold()
        );
        println!(
            "  Try broader terms or run {} first.",
            "testforge index .".cyan()
        );
        return Ok(());
    }

    println!(
        "\n  Found {} results for \"{}\"\n",
        filtered.len().to_string().cyan(),
        args.query.bold()
    );

    for (i, sym) in filtered.iter().enumerate() {
        let kind_badge = format!(" {} ", sym.kind).on_blue().white().bold();
        let vis = match sym.visibility {
            testforge_core::models::Visibility::Private => " private".dimmed().to_string(),
            testforge_core::models::Visibility::Protected => " protected".dimmed().to_string(),
            _ => String::new(),
        };

        println!(
            "  {} {}{} {}",
            format!("{:>2}.", i + 1).dimmed(),
            kind_badge,
            vis,
            sym.qualified_name.bold()
        );

        println!(
            "     {} {}:{}–{}",
            "↳".dimmed(),
            sym.file_path.display().to_string().underline(),
            sym.start_line,
            sym.end_line
        );

        if let Some(ref sig) = sym.signature {
            let truncated = if sig.len() > 80 {
                format!("{}…", &sig[..77])
            } else {
                sig.clone()
            };
            println!("     {}", truncated.dimmed());
        }

        if let Some(ref doc) = sym.docstring {
            let truncated = if doc.len() > 80 {
                format!("{}…", &doc[..77])
            } else {
                doc.clone()
            };
            println!("     {}", truncated.italic().dimmed());
        }

        println!();
    }

    Ok(())
}

/// Simple relevance scoring for keyword search.
fn search_score(sym: &testforge_core::models::Symbol, query: &str) -> f64 {
    let mut score = 0.0;

    // Exact name match
    if sym.name.to_lowercase() == query {
        score += 10.0;
    }
    // Name starts with query
    else if sym.name.to_lowercase().starts_with(query) {
        score += 7.0;
    }
    // Name contains query
    else if sym.name.to_lowercase().contains(query) {
        score += 5.0;
    }

    // Qualified name match
    if sym.qualified_name.to_lowercase().contains(query) {
        score += 3.0;
    }

    // Docstring match
    if sym
        .docstring
        .as_ref()
        .map(|d| d.to_lowercase().contains(query))
        .unwrap_or(false)
    {
        score += 2.0;
    }

    // Prefer public symbols
    if sym.visibility == testforge_core::models::Visibility::Public {
        score += 0.5;
    }

    // Prefer shorter symbols (more focused)
    score += 1.0 / (sym.line_count() as f64).max(1.0);

    score
}
