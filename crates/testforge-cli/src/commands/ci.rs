//! `testforge ci` — CI/CD integration mode.
//!
//! Runs TestForge in headless mode suitable for CI pipelines:
//! - Indexes the project
//! - Analyzes test coverage gaps
//! - Optionally generates tests for uncovered functions
//! - Outputs a JSON report for CI artifact collection
//!
//! Exit codes:
//! - 0: all checks passed
//! - 1: coverage below threshold (configurable)
//! - 2: errors during analysis

use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use colored::Colorize;
use testforge_core::models::SymbolKind;
use testforge_core::Config;
use testforge_indexer::Indexer;

#[derive(Args)]
pub struct CiArgs {
    /// Path to analyze (defaults to project root)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Minimum coverage ratio (0.0-1.0) to pass
    #[arg(long, default_value = "0.0")]
    min_coverage: f64,

    /// Output format: "pretty" or "json"
    #[arg(short, long, default_value = "pretty")]
    format: String,

    /// Output report to file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Generate tests for uncovered functions
    #[arg(long)]
    generate: bool,

    /// Maximum functions to generate tests for
    #[arg(long, default_value = "10")]
    generate_limit: usize,

    /// Fail CI if coverage is below threshold
    #[arg(long)]
    strict: bool,
}

#[derive(serde::Serialize)]
struct CiReport {
    project: String,
    timestamp: String,
    summary: CiSummary,
    uncovered: Vec<UncoveredSymbol>,
    coverage_by_file: Vec<FileCoverage>,
}

#[derive(serde::Serialize)]
struct CiSummary {
    total_functions: usize,
    tested_functions: usize,
    untested_functions: usize,
    coverage_ratio: f64,
    pass: bool,
    threshold: f64,
}

#[derive(serde::Serialize)]
struct UncoveredSymbol {
    name: String,
    qualified_name: String,
    kind: String,
    file: String,
    lines: String,
    complexity_risk: String,
}

#[derive(serde::Serialize)]
struct FileCoverage {
    file: String,
    total: usize,
    tested: usize,
    coverage: f64,
}

pub fn run(args: CiArgs) -> anyhow::Result<()> {
    let start = Instant::now();
    let (config, project_root) = Config::discover(&args.path)?;
    let mut indexer = Indexer::new(config, &project_root)?;

    // Step 1: Index
    if args.format == "pretty" {
        println!("  {} Indexing project...", "→".blue());
    }

    let _index_report = indexer.index_full()?;
    let all_symbols = indexer.all_symbols()?;

    // Step 2: Identify testable functions
    let testable: Vec<_> = all_symbols
        .iter()
        .filter(|s| {
            matches!(s.kind, SymbolKind::Function | SymbolKind::Method) && s.line_count() >= 3
        })
        .collect();

    // Step 3: Find test files and map coverage
    let test_symbols: Vec<_> = all_symbols
        .iter()
        .filter(|s| {
            let path = s.file_path.to_string_lossy().to_lowercase();
            path.contains("test_")
                || path.contains("_test.")
                || path.contains(".test.")
                || path.contains("tests/")
                || path.contains("spec/")
        })
        .collect();

    // Build a set of "tested" function names by analyzing test source
    let test_sources: String = test_symbols
        .iter()
        .map(|s| s.source.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let tested: Vec<_> = testable
        .iter()
        .filter(|s| test_sources.contains(&s.name))
        .cloned()
        .collect();

    let untested: Vec<_> = testable
        .iter()
        .filter(|s| !test_sources.contains(&s.name))
        .cloned()
        .collect();

    let coverage = if testable.is_empty() {
        0.0
    } else {
        tested.len() as f64 / testable.len() as f64
    };

    let pass = coverage >= args.min_coverage;

    // Step 4: Build per-file coverage
    let mut file_map: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for sym in &testable {
        let file = sym.file_path.to_string_lossy().to_string();
        let entry = file_map.entry(file).or_insert((0, 0));
        entry.0 += 1;
        if test_sources.contains(&sym.name) {
            entry.1 += 1;
        }
    }

    let mut coverage_by_file: Vec<FileCoverage> = file_map
        .into_iter()
        .map(|(file, (total, tested))| FileCoverage {
            file,
            total,
            tested,
            coverage: if total > 0 {
                tested as f64 / total as f64
            } else {
                1.0
            },
        })
        .collect();
    coverage_by_file.sort_by(|a, b| {
        a.coverage
            .partial_cmp(&b.coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Step 5: Build report
    let uncovered_list: Vec<UncoveredSymbol> = untested
        .iter()
        .take(50)
        .map(|s| UncoveredSymbol {
            name: s.name.clone(),
            qualified_name: s.qualified_name.clone(),
            kind: s.kind.to_string(),
            file: s.file_path.to_string_lossy().to_string(),
            lines: format!("{}-{}", s.start_line, s.end_line),
            complexity_risk: if s.line_count() > 30 {
                "high".into()
            } else if s.line_count() > 15 {
                "medium".into()
            } else {
                "low".into()
            },
        })
        .collect();

    let report = CiReport {
        project: project_root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        summary: CiSummary {
            total_functions: testable.len(),
            tested_functions: tested.len(),
            untested_functions: untested.len(),
            coverage_ratio: (coverage * 100.0).round() / 100.0,
            pass,
            threshold: args.min_coverage,
        },
        uncovered: uncovered_list,
        coverage_by_file,
    };

    // Step 6: Output
    if args.format == "json" {
        let json = serde_json::to_string_pretty(&report)?;
        if let Some(ref output_path) = args.output {
            std::fs::write(output_path, &json)?;
            eprintln!("Report written to {}", output_path.display());
        } else {
            println!("{json}");
        }
    } else {
        println!();
        println!("  {} TestForge CI Report", "◆".cyan().bold());
        println!();
        println!(
            "  Functions:    {} total, {} tested, {} uncovered",
            testable.len().to_string().cyan(),
            tested.len().to_string().green(),
            untested.len().to_string().yellow(),
        );
        println!(
            "  Coverage:     {}",
            if pass {
                format!("{:.0}% ✓", coverage * 100.0).green().bold()
            } else {
                format!(
                    "{:.0}% ✗ (threshold: {:.0}%)",
                    coverage * 100.0,
                    args.min_coverage * 100.0
                )
                .red()
                .bold()
            }
        );
        println!("  Duration:     {:.1}s", start.elapsed().as_secs_f64());

        if !untested.is_empty() {
            println!();
            println!("  {} Uncovered functions (top 15):", "△".yellow());
            for sym in untested.iter().take(15) {
                let risk = if sym.line_count() > 30 {
                    "HIGH".red()
                } else if sym.line_count() > 15 {
                    "MED".yellow()
                } else {
                    "LOW".dimmed()
                };
                println!(
                    "    {} {} {} ({}:{})",
                    risk,
                    sym.kind.to_string().dimmed(),
                    sym.qualified_name.bold(),
                    sym.file_path.display(),
                    sym.start_line
                );
            }
        }

        if !report.coverage_by_file.is_empty() {
            println!();
            println!("  {} Coverage by file (lowest first):", "📊".bold());
            for fc in report.coverage_by_file.iter().take(10) {
                let bar_len = (fc.coverage * 20.0) as usize;
                let bar = format!("{}{}", "█".repeat(bar_len), "░".repeat(20 - bar_len));
                println!(
                    "    {} {:.0}% {}",
                    bar,
                    fc.coverage * 100.0,
                    fc.file.dimmed()
                );
            }
        }

        println!();

        // Write JSON if output specified
        if let Some(ref output_path) = args.output {
            let json = serde_json::to_string_pretty(&report)?;
            std::fs::write(output_path, &json)?;
            println!(
                "  Report: {}",
                output_path.display().to_string().underline()
            );
        }
    }

    if args.strict && !pass {
        std::process::exit(1);
    }

    Ok(())
}
