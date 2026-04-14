//! `testforge gen-tests` — generate tests for functions, methods, or entire files.
//!
//! This command bridges the Rust CLI with the Python AI layer:
//! 1. Resolves the target symbol(s) from the index
//! 2. Delegates to `testforge-ai gen` for LLM-powered generation
//! 3. Writes the generated test files to the output directory
use std::process::Command as ProcessCommand;
use std::time::Instant;

use clap::Args;
use colored::Colorize;
use testforge_core::models::SymbolKind;
use testforge_core::Config;
use testforge_indexer::Indexer;

#[derive(Args)]
pub struct GenTestsArgs {
    /// Target: file path, qualified symbol name, or file::symbol
    ///
    /// Examples:
    ///   src/auth.py                    — all public functions in file
    ///   src/auth.py::AuthService       — specific class
    ///   AuthService.authenticate       — by qualified name
    target: String,

    /// Process all symbols in the directory recursively
    #[arg(short, long)]
    recursive: bool,

    /// Test framework override (e.g., "pytest", "jest", "cargo-test")
    #[arg(long)]
    framework: Option<String>,

    /// Include edge case analysis
    #[arg(long, default_value = "true")]
    edge_cases: bool,

    /// Include mock/stub generation
    #[arg(long, default_value = "true")]
    mocks: bool,

    /// Output directory for generated tests
    #[arg(short, long)]
    output: Option<String>,

    /// LLM provider override ("claude", "openai", "local")
    #[arg(long)]
    provider: Option<String>,

    /// LLM model override
    #[arg(long)]
    model: Option<String>,

    /// Preview generated tests without writing to disk
    #[arg(long)]
    dry_run: bool,

    /// Maximum tokens for the LLM response
    #[arg(long, default_value = "4096")]
    max_tokens: usize,
}

pub fn run(args: GenTestsArgs) -> anyhow::Result<()> {
    let start = Instant::now();
    let cwd = std::env::current_dir()?;
    let (config, project_root) = Config::discover(&cwd)?;
    let indexer = Indexer::new(config.clone(), &project_root)?;

    // Resolve target symbols
    let symbols = resolve_targets(&args, &indexer)?;

    if symbols.is_empty() {
        println!(
            "  {} No matching symbols found for \"{}\"",
            "✗".red(),
            args.target.bold()
        );
        println!(
            "  Run {} to see indexed symbols.",
            "testforge search <name>".cyan()
        );
        return Ok(());
    }

    println!(
        "\n  {} Generating tests for {} symbol(s)\n",
        "⚡".bold(),
        symbols.len().to_string().cyan()
    );

    // Display targets
    for sym in &symbols {
        println!(
            "    {} {} {} ({}:{}–{})",
            "→".blue(),
            format!("{}", sym.kind).dimmed(),
            sym.qualified_name.bold(),
            sym.file_path.display(),
            sym.start_line,
            sym.end_line,
        );
    }
    println!();

    // Determine output directory
    let output_dir = args
        .output
        .clone()
        .unwrap_or_else(|| config.generation.output_dir.clone());
    let output_path = project_root.join(&output_dir);

    if !args.dry_run {
        std::fs::create_dir_all(&output_path)?;
    }

    let framework = args
        .framework
        .clone()
        .unwrap_or_else(|| config.generation.test_framework.clone());

    let provider = args
        .provider
        .clone()
        .unwrap_or_else(|| config.llm.provider.clone());

    // Invoke the Python AI layer for each symbol
    let mut generated = 0;
    let mut failed = 0;

    for sym in &symbols {
        let target_arg = &sym.qualified_name;

        println!(
            "  {} Generating tests for {}...",
            "⏳".dimmed(),
            sym.qualified_name.cyan()
        );

        // Build Python command
        let mut cmd = ProcessCommand::new("python3");
        cmd.arg("-m")
            .arg("testforge_ai.cli_gen")
            .arg("--project")
            .arg(project_root.to_str().unwrap_or("."))
            .arg("--target")
            .arg(target_arg)
            .arg("--framework")
            .arg(&framework)
            .arg("--provider")
            .arg(&provider)
            .arg("--max-tokens")
            .arg(args.max_tokens.to_string());

        if args.edge_cases {
            cmd.arg("--edge-cases");
        }
        if args.mocks {
            cmd.arg("--mocks");
        }
        if args.dry_run {
            cmd.arg("--dry-run");
        }
        if let Some(ref model) = args.model {
            cmd.arg("--model").arg(model);
        }
        if !args.dry_run {
            cmd.arg("--output-dir")
                .arg(output_path.to_str().unwrap_or("."));
        }

        // Set PYTHONPATH to include the project's python/ directory
        let python_path = project_root.join("python");
        cmd.env("PYTHONPATH", python_path.to_str().unwrap_or(""));

        let result = cmd.output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    generated += 1;

                    // Parse and display result
                    if args.dry_run {
                        println!("\n{}\n", stdout);
                    } else {
                        // Try to extract the output filename from stdout
                        for line in stdout.lines() {
                            if line.contains("Written to:") || line.contains("wrote") {
                                println!("    {} {}", "✓".green(), line.trim());
                            }
                        }
                        if stdout.contains("test_count") {
                            // JSON output
                            if let Ok(info) = serde_json::from_str::<serde_json::Value>(&stdout) {
                                let count =
                                    info.get("test_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let file = info
                                    .get("file_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                println!(
                                    "    {} Generated {} tests → {}",
                                    "✓".green().bold(),
                                    count.to_string().cyan(),
                                    output_path.join(file).display().to_string().underline()
                                );
                            }
                        } else {
                            println!("    {} Tests generated", "✓".green().bold());
                        }
                    }

                    if !stderr.is_empty() {
                        for line in stderr.lines() {
                            if line.contains("WARNING") || line.contains("WARN") {
                                println!("    {} {}", "⚠".yellow(), line.trim().dimmed());
                            }
                        }
                    }
                } else {
                    failed += 1;
                    println!("    {} Failed for {}", "✗".red(), sym.qualified_name.red());
                    if !stderr.is_empty() {
                        for line in stderr.lines().take(5) {
                            println!("      {}", line.dimmed());
                        }
                    }
                }
            }
            Err(e) => {
                failed += 1;
                if e.kind() == std::io::ErrorKind::NotFound {
                    println!(
                        "    {} Python not found. Install the AI layer: {}",
                        "✗".red(),
                        "pip install -e '.[dev]'".cyan()
                    );
                    break;
                }
                println!("    {} Error: {}", "✗".red(), e);
            }
        }
    }

    // Summary
    let elapsed = start.elapsed();
    println!();
    println!(
        "  {} Done in {:.1}s — {} generated, {} failed",
        "◆".cyan().bold(),
        elapsed.as_secs_f64(),
        generated.to_string().green(),
        if failed > 0 {
            failed.to_string().red()
        } else {
            failed.to_string().dimmed()
        },
    );

    if !args.dry_run && generated > 0 {
        println!(
            "  Output: {}",
            output_path.display().to_string().underline()
        );
    }
    println!();

    Ok(())
}

/// Resolve the target argument into a list of symbols.
fn resolve_targets(
    args: &GenTestsArgs,
    indexer: &Indexer,
) -> anyhow::Result<Vec<testforge_core::models::Symbol>> {
    let all = indexer.all_symbols()?;
    let target = &args.target;

    // Case 1: file::symbol notation
    if target.contains("::") {
        let parts: Vec<&str> = target.splitn(2, "::").collect();
        let file = parts[0];
        let sym_name = parts[1];

        let matches: Vec<_> = all
            .into_iter()
            .filter(|s| {
                s.file_path.to_string_lossy().contains(file)
                    && (s.name == sym_name || s.qualified_name == sym_name)
            })
            .collect();

        return Ok(matches);
    }

    // Case 2: file path (all public symbols in file)
    if target.contains('/') || target.contains('.') {
        let path_target = std::path::Path::new(target);

        if args.recursive && path_target.is_dir() {
            // All symbols under the directory
            let matches: Vec<_> = all
                .into_iter()
                .filter(|s| {
                    s.file_path.to_string_lossy().starts_with(target.as_str()) && is_testable(s)
                })
                .collect();
            return Ok(matches);
        }

        // Single file
        let matches: Vec<_> = all
            .into_iter()
            .filter(|s| s.file_path.to_string_lossy().contains(target.as_str()) && is_testable(s))
            .collect();
        return Ok(matches);
    }

    // Case 3: qualified symbol name
    let matches: Vec<_> = all
        .into_iter()
        .filter(|s| {
            s.qualified_name == *target
                || s.name == *target
                || s.qualified_name.ends_with(&format!(".{}", target))
                || s.qualified_name.ends_with(&format!("::{}", target))
        })
        .collect();

    Ok(matches)
}

/// Check if a symbol is worth generating tests for.
fn is_testable(sym: &testforge_core::models::Symbol) -> bool {
    matches!(sym.kind, SymbolKind::Function | SymbolKind::Method)
        && sym.visibility == testforge_core::models::Visibility::Public
        && sym.line_count() >= 3
}
