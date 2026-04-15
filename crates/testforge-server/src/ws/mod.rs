//! WebSocket endpoints for real-time progress streaming.
//!
//! Clients connect to `/ws/progress/:job_id` and receive JSON-encoded
//! progress updates as the job runs.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tracing::{debug, warn};

use crate::state::AppState;

/// Build the WebSocket router.
pub fn ws_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws/progress/{job_id}", get(ws_progress))
        .with_state(state)
}

/// WebSocket handler for job progress.
async fn ws_progress(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_progress_ws(socket, state, job_id))
}

/// Handle a WebSocket connection for job progress.
async fn handle_progress_ws(mut socket: WebSocket, state: Arc<AppState>, job_id: String) {
    debug!(job_id = %job_id, "WebSocket client connected");

    // Subscribe to the job's progress channel
    let maybe_rx = {
        let jobs = state.jobs.read().await;
        jobs.get(&job_id).map(|tx| tx.subscribe())
    };

    let Some(mut rx) = maybe_rx else {
        // Job not found or already completed
        let msg = serde_json::json!({
            "type": "error",
            "message": format!("Job '{}' not found or already completed", job_id)
        });
        let _ = socket.send(Message::Text(msg.to_string())).await;
        let _ = socket.close().await;
        return;
    };

    // Forward progress events to the WebSocket client
    loop {
        match rx.recv().await {
            Ok(progress) => {
                let json = match serde_json::to_string(&progress) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Failed to serialize progress: {}", e);
                        continue;
                    }
                };

                if socket.send(Message::Text(json)).await.is_err() {
                    debug!(job_id = %job_id, "WebSocket client disconnected");
                    break;
                }

                // Close after completion or error
                match progress {
                    crate::state::JobProgress::Complete { .. }
                    | crate::state::JobProgress::Error { .. } => {
                        debug!(job_id = %job_id, "Job finished, closing WebSocket");
                        let _ = socket.close().await;
                        break;
                    }
                    _ => {}
                }
            }
            Err(_) => {
                // Channel closed = job finished
                debug!(job_id = %job_id, "Progress channel closed");
                break;
            }
        }
    }
}
