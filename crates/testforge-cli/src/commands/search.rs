//! `testforge search` — search the codebase using natural language or keywords.
//!
//! Uses the hybrid search engine (tantivy full-text + vector cosine similarity)
//! when embeddings are available, gracefully falling back to full-text only.

use clap::Args;
use colored::Colorize;
use testforge_core::models::Language;
use testforge_core::Config;
use testforge_search::{ranking, SearchEngine, SearchQuery};

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

    /// Filter by file path prefix
    #[arg(short, long)]
    path: Option<String>,

    /// Semantic weight: 0.0 = text only, 1.0 = semantic only (default 0.6)
    #[arg(short, long, default_value = "0.6")]
    semantic_weight: f32,

    /// Output format: "pretty" (default) or "json"
    #[arg(short, long, default_value = "pretty")]
    format: String,

    /// Show ranking explanation for each result
    #[arg(long)]
    explain: bool,
}

pub fn run(args: SearchArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (config, project_root) = Config::discover(&cwd)?;

    let search_dir = project_root
        .join(testforge_core::config::CONFIG_DIR)
        .join("search");

    // Build search query
    let mut query = SearchQuery::new(&args.query)
        .with_limit(args.limit)
        .with_semantic_weight(args.semantic_weight);

    if let Some(ref lang) = args.language {
        if let Some(l) = parse_language(lang) {
            query = query.with_language(l);
        }
    }

    if let Some(ref kind) = args.kind {
        query = query.with_kind(kind.to_lowercase());
    }

    if let Some(ref path) = args.path {
        query = query.with_path_prefix(path.clone());
    }

    // Try the search engine first (tantivy + vectors)
    let mut results = if search_dir.join("tantivy").exists() {
        let engine = SearchEngine::open(&search_dir, &config)?;

        // TODO: compute query embedding via Python bridge and pass it here
        // For now, text-only search
        let mut res = engine.search(&query, None)?;

        // Apply re-ranking pipeline
        ranking::rerank(&mut res);
        apply_query_match_boost(&mut res, &args.query);
        // Sort by score after reranking
        res.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranking::deduplicate(&mut res);
        ranking::diversify(&mut res, 5);
        res.truncate(args.limit);
        res
    } else {
        Vec::new()
    };

    // Fallback to SQLite index if the search engine is empty
    if results.is_empty() {
        let indexer = testforge_indexer::Indexer::new(config.clone(), &project_root)?;
        let all_symbols = indexer.all_symbols()?;

        let query_lower = args.query.to_lowercase();
        let mut fallback: Vec<_> = all_symbols
            .into_iter()
            .filter(|sym| {
                sym.name.to_lowercase().contains(&query_lower)
                    || sym.qualified_name.to_lowercase().contains(&query_lower)
                    || sym
                        .docstring
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
                    || sym.source.to_lowercase().contains(&query_lower)
            })
            .filter(|sym| {
                if let Some(ref lang) = args.language {
                    sym.language.to_string() == lang.to_lowercase()
                } else {
                    true
                }
            })
            .filter(|sym| {
                if let Some(ref kind) = args.kind {
                    sym.kind.to_string() == kind.to_lowercase()
                } else {
                    true
                }
            })
            .map(|sym| testforge_core::models::SearchResult {
                symbol: sym,
                score: 0.5,
                match_source: testforge_core::models::MatchSource::FullText,
            })
            .collect();

        ranking::rerank(&mut fallback);
        apply_query_match_boost(&mut fallback, &args.query);
        // Sort by score after reranking
        fallback.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fallback.truncate(args.limit);
        results = fallback;
    }

    // Output
    if args.format == "json" {
        // For JSON output, serialize just the symbols (backward compatible)
        let symbols: Vec<_> = results.iter().map(|r| &r.symbol).collect();
        println!("{}", serde_json::to_string_pretty(&symbols)?);
        return Ok(());
    }

    if results.is_empty() {
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
        results.len().to_string().cyan(),
        args.query.bold()
    );

    for (i, result) in results.iter().enumerate() {
        let sym = &result.symbol;

        let source_badge = match result.match_source {
            testforge_core::models::MatchSource::Semantic => " semantic ".on_magenta().white(),
            testforge_core::models::MatchSource::FullText => " text ".on_blue().white(),
            testforge_core::models::MatchSource::Hybrid => " hybrid ".on_green().white(),
        };

        let kind_badge = format!(" {} ", sym.kind).on_bright_black().white().bold();
        let score_str = format!("{:.3}", result.score).dimmed();

        println!(
            "  {} {} {} {}  {}",
            format!("{:>2}.", i + 1).dimmed(),
            kind_badge,
            source_badge,
            sym.qualified_name.bold(),
            score_str,
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

        if args.explain {
            let explanation = ranking::explain_ranking(result, &args.query);
            println!("     {} {}", "?".yellow(), explanation.dimmed());
        }

        println!();
    }

    Ok(())
}

fn apply_query_match_boost(results: &mut [testforge_core::models::SearchResult], query: &str) {
    let query_lower = query.to_lowercase();

    for result in results.iter_mut() {
        let name = result.symbol.name.to_lowercase();
        let qualified = result.symbol.qualified_name.to_lowercase();

        let multiplier = if name == query_lower || qualified == query_lower {
            3.0
        } else if qualified.ends_with(&format!(".{query_lower}")) {
            2.5
        } else if name.starts_with(&query_lower) {
            1.8
        } else if name.contains(&query_lower) || qualified.contains(&query_lower) {
            1.2
        } else {
            1.0
        };

        result.score *= multiplier;
    }
}

fn parse_language(s: &str) -> Option<Language> {
    Language::from_extension(s).or_else(|| match s.to_lowercase().as_str() {
        "python" => Some(Language::Python),
        "javascript" | "js" => Some(Language::JavaScript),
        "typescript" | "ts" => Some(Language::TypeScript),
        "rust" => Some(Language::Rust),
        "java" => Some(Language::Java),
        "go" => Some(Language::Go),
        _ => None,
    })
}
