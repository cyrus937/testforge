//! # TestForge Server
//!
//! REST API server providing programmatic access to TestForge's indexing,
//! search, and test generation capabilities. Built with [Axum](https://github.com/tokio-rs/axum).
//!
//! ## Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | `/api/health` | Server health check |
//! | GET | `/api/status` | Index statistics |
//! | POST | `/api/search` | Hybrid search |
//! | POST | `/api/index` | Trigger indexing |
//! | POST | `/api/generate-tests` | Generate tests |
//! | GET | `/api/symbols` | List indexed symbols |
//! | WS | `/ws/progress/:job_id` | Real-time progress |

pub mod routes;
pub mod state;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use testforge_core::{Config, Result};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use state::AppState;

/// Server configuration.
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors: bool,
    pub project_root: std::path::PathBuf,
    pub config: Config,
}

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let api = routes::api_router(state.clone());

    let mut app = Router::new()
        .nest("/api", api)
        .merge(ws::ws_router(state.clone()))
        .layer(TraceLayer::new_for_http());

    // CORS
    if state.config.server.cors {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        app = app.layer(cors);
    }

    app
}

/// Start the server and block until shutdown.
pub async fn run(server_config: ServerConfig) -> Result<()> {
    let state = AppState::new(server_config.config.clone(), &server_config.project_root)?;
    let state = Arc::new(state);

    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", server_config.host, server_config.port)
        .parse()
        .map_err(|e| testforge_core::TestForgeError::internal(format!("Invalid address: {e}")))?;

    info!("TestForge API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| testforge_core::TestForgeError::internal(format!("Bind failed: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| testforge_core::TestForgeError::internal(format!("Server error: {e}")))?;

    Ok(())
}
