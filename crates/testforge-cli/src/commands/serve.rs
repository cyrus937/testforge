//! `testforge serve` — start the REST API server.

use clap::Args;
use colored::Colorize;
use testforge_core::Config;

#[derive(Args)]
pub struct ServeArgs {
    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port number
    #[arg(short, long, default_value = "7654")]
    port: u16,

    /// Disable CORS headers
    #[arg(long)]
    no_cors: bool,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (mut config, project_root) = Config::discover(&cwd)?;

    // Override config with CLI args
    config.server.host = args.host.clone();
    config.server.port = args.port;
    config.server.cors = !args.no_cors;

    println!();
    println!("  {} TestForge API Server", "⚡".bold());
    println!();
    println!("  Project:  {}", project_root.display().to_string().cyan());
    println!(
        "  Address:  {}",
        format!("http://{}:{}", args.host, args.port).green().bold()
    );
    println!(
        "  CORS:     {}",
        if !args.no_cors {
            "enabled".green()
        } else {
            "disabled".yellow()
        }
    );
    println!();
    println!("  Endpoints:");
    println!("    GET  /api/health              Health check");
    println!("    GET  /api/status              Index statistics");
    println!("    POST /api/search              Hybrid search");
    println!("    POST /api/index               Trigger indexing");
    println!("    POST /api/generate-tests      Generate tests");
    println!("    GET  /api/symbols             List symbols");
    println!("    WS   /ws/progress/{{job_id}}    Progress stream");
    println!();
    println!("  Press {} to stop.", "Ctrl+C".bold());
    println!();

    let server_config = testforge_server::ServerConfig {
        host: args.host,
        port: args.port,
        cors: !args.no_cors,
        project_root,
        config,
    };

    testforge_server::run(server_config).await?;

    Ok(())
}
