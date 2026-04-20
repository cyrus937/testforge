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

    // Normalize the target path (strip leading ./)
    let normalized_target = target.strip_prefix("./").unwrap_or(target).to_string();

    // Case 1: file::symbol notation
    if normalized_target.contains("::") {
        let parts: Vec<&str> = normalized_target.splitn(2, "::").collect();
        let file = parts[0];
        let sym_name = parts[1];

        let matches: Vec<_> = all
            .into_iter()
            .filter(|s| {
                path_matches(&s.file_path, file)
                    && (s.name == sym_name
                        || s.qualified_name == sym_name
                        || s.qualified_name.ends_with(&format!(".{sym_name}"))
                        || s.qualified_name.ends_with(&format!("::{sym_name}")))
            })
            .collect();

        if matches.is_empty() {
            print_diagnostic(&normalized_target, indexer);
        }

        return Ok(matches);
    }

    // Case 2: looks like a file path
    if normalized_target.contains('/') || normalized_target.contains('.') {
        let path_target = std::path::Path::new(&normalized_target);

        if args.recursive && path_target.is_dir() {
            let matches: Vec<_> = all
                .into_iter()
                .filter(|s| {
                    let sp = s.file_path.to_string_lossy();
                    (sp.starts_with(&normalized_target) || sp.contains(&normalized_target))
                        && is_testable_strict(s)
                })
                .collect();

            if matches.is_empty() {
                print_diagnostic(&normalized_target, indexer);
            }
            return Ok(matches);
        }

        // Single file — use relaxed filter (include private functions, short functions)
        // When the user explicitly targets a file, they want everything in it.
        let matches: Vec<_> = all
            .into_iter()
            .filter(|s| path_matches(&s.file_path, &normalized_target) && is_testable_relaxed(s))
            .collect();

        if matches.is_empty() {
            print_diagnostic(&normalized_target, indexer);
        }

        return Ok(matches);
    }

    // Case 3: qualified symbol name (no path separator, no dots-with-extension)
    let matches: Vec<_> = all
        .into_iter()
        .filter(|s| {
            s.qualified_name == normalized_target
                || s.name == normalized_target
                || s.qualified_name
                    .ends_with(&format!(".{}", normalized_target))
                || s.qualified_name
                    .ends_with(&format!("::{}", normalized_target))
        })
        .collect();

    if matches.is_empty() {
        print_diagnostic(&normalized_target, indexer);
    }

    Ok(matches)
}

/// Check if a file path matches a target pattern.
///
/// Handles various path formats:
/// - `src/main.rs` matches `src/main.rs`
/// - `main.rs` matches `src/main.rs` (suffix match)
/// - `src/auth/` matches `src/auth/service.py` (prefix match)
fn path_matches(file_path: &std::path::Path, target: &str) -> bool {
    let file_str = file_path.to_string_lossy();
    let normalized = file_str.strip_prefix("./").unwrap_or(&file_str);

    // Exact match
    if normalized == target {
        return true;
    }

    // Target is a suffix (e.g., "main.rs" matches "src/main.rs")
    if normalized.ends_with(target) {
        // Ensure we match at a path boundary
        let prefix_end = normalized.len() - target.len();
        if prefix_end == 0 || normalized.as_bytes()[prefix_end - 1] == b'/' {
            return true;
        }
    }

    // Target is a prefix (directory match, e.g., "src/auth" matches "src/auth/service.py")
    if normalized.starts_with(target) {
        return true;
    }

    // Contains match (e.g., "auth/service" matches "src/auth/service.py")
    if normalized.contains(target) {
        return true;
    }

    false
}

/// Strict testability check — for recursive/automatic discovery.
///
/// Only includes public functions/methods with enough substance to test.
fn is_testable_strict(sym: &testforge_core::models::Symbol) -> bool {
    matches!(sym.kind, SymbolKind::Function | SymbolKind::Method)
        && sym.visibility == testforge_core::models::Visibility::Public
        && sym.line_count() >= 3
}

/// Relaxed testability check — for explicit file/symbol targets.
///
/// Includes all functions and methods regardless of visibility or size.
/// When the user explicitly asks for tests for a file, they want all of it.
/// The only things we exclude are classes/structs/enums (type definitions)
/// and trivially empty functions (1 line = just the signature).
fn is_testable_relaxed(sym: &testforge_core::models::Symbol) -> bool {
    matches!(sym.kind, SymbolKind::Function | SymbolKind::Method) && sym.line_count() >= 2
}

/// Print diagnostic information when no symbols are found.
///
/// Helps the user understand why their target didn't match:
/// - Are there any indexed files at all?
/// - Is the file indexed but with no matching symbols?
/// - Did the path not match any indexed file?
fn print_diagnostic(target: &str, indexer: &Indexer) {
    let all = match indexer.all_symbols() {
        Ok(s) => s,
        Err(_) => return,
    };

    if all.is_empty() {
        println!(
            "  {} The index is empty. Run {} first.",
            "ℹ".blue(),
            "testforge index .".cyan()
        );
        return;
    }

    // Collect unique indexed file paths
    let mut indexed_files: Vec<String> = all
        .iter()
        .map(|s| s.file_path.to_string_lossy().to_string())
        .collect();
    indexed_files.sort();
    indexed_files.dedup();

    // Check if any file partially matches
    let partial_matches: Vec<_> = indexed_files
        .iter()
        .filter(|f| {
            f.contains(target)
                || target.contains(f.as_str())
                || f.to_lowercase().contains(&target.to_lowercase())
        })
        .collect();

    if !partial_matches.is_empty() {
        println!("  {} Similar indexed files found:", "ℹ".blue());
        for f in partial_matches.iter().take(5) {
            let syms_in_file: Vec<_> = all
                .iter()
                .filter(|s| s.file_path.to_string_lossy() == **f)
                .collect();
            println!(
                "    {} ({} symbols: {})",
                f.to_string().underline(),
                syms_in_file.len(),
                syms_in_file
                    .iter()
                    .take(5)
                    .map(|s| format!("{} {}", s.kind, s.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
        println!(
            "  {} Check the exact path with: {}",
            "💡".bold(),
            "testforge search <keyword> --format json | jq '.[].file_path'".dimmed()
        );
    } else {
        // No match at all — show some indexed files for reference
        println!(
            "  {} {} indexed files, but none matching \"{}\".",
            "ℹ".blue(),
            indexed_files.len(),
            target
        );
        println!("  Indexed files (first 10):");
        for f in indexed_files.iter().take(10) {
            println!("    {}", f.dimmed());
        }
        if indexed_files.len() > 10 {
            println!("    ... and {} more", indexed_files.len() - 10);
        }
    }
}
