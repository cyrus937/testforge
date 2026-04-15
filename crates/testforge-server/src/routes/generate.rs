//! Test generation endpoint — delegates to Python AI layer.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::health::ErrorResponse;
use crate::state::{AppState, JobProgress};

#[derive(Deserialize)]
pub struct GenerateRequest {
    pub target: String,
    #[serde(default = "default_framework")]
    pub framework: String,
    #[serde(default)]
    pub include_edge_cases: bool,
    #[serde(default)]
    pub include_mocks: bool,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_framework() -> String {
    "pytest".into()
}
fn default_max_tokens() -> usize {
    4096
}
fn default_temperature() -> f32 {
    0.2
}

#[derive(Serialize)]
pub struct GenerateJobResponse {
    pub job_id: String,
    pub status: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct GenerateResultResponse {
    pub job_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<GenerateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct GenerateResult {
    pub source: String,
    pub file_name: String,
    pub target_symbol: String,
    pub test_count: usize,
    pub framework: String,
    pub warnings: Vec<String>,
}

/// Completed generation results, stored in memory.
static COMPLETED_JOBS: std::sync::LazyLock<
    RwLock<std::collections::HashMap<String, GenerateResultResponse>>,
> = std::sync::LazyLock::new(|| RwLock::new(std::collections::HashMap::new()));

/// `POST /api/generate-tests` — trigger async test generation.
pub async fn generate_tests(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Result<(StatusCode, Json<GenerateJobResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Verify target exists
    let target_name = req.target.clone();
    {
        let indexer = state.indexer.lock().await;
        let symbols = indexer.all_symbols().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::internal(&e.to_string())),
            )
        })?;

        let found = symbols
            .iter()
            .any(|s| s.qualified_name == target_name || s.name == target_name);

        if !found {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::not_found(&format!(
                    "Symbol '{}' not found in index",
                    target_name
                ))),
            ));
        }
    }

    let (job_id, tx) = state.create_job("gen").await;

    // Spawn async generation
    let state_clone = state.clone();
    let job_id_clone = job_id.clone();

    tokio::spawn(async move {
        let _ = tx.send(JobProgress::Progress {
            message: format!("Generating tests for {}...", req.target),
            percent: Some(10.0),
            current_item: Some(req.target.clone()),
        });

        // Invoke Python backend
        let result = invoke_python_gen(
            &state_clone.project_root,
            &req.target,
            &req.framework,
            req.include_edge_cases,
            req.include_mocks,
            req.max_tokens,
        )
        .await;

        let response = match result {
            Ok(gen_result) => {
                let _ = tx.send(JobProgress::Complete {
                    message: format!(
                        "Generated {} tests for {}",
                        gen_result.test_count, gen_result.target_symbol
                    ),
                    result: serde_json::to_value(&gen_result).unwrap_or_default(),
                });

                GenerateResultResponse {
                    job_id: job_id_clone.clone(),
                    status: "complete".into(),
                    result: Some(gen_result),
                    error: None,
                }
            }
            Err(e) => {
                let _ = tx.send(JobProgress::Error { message: e.clone() });

                GenerateResultResponse {
                    job_id: job_id_clone.clone(),
                    status: "failed".into(),
                    result: None,
                    error: Some(e),
                }
            }
        };

        // Store result
        COMPLETED_JOBS
            .write()
            .await
            .insert(job_id_clone.clone(), response);
        state_clone.remove_job(&job_id_clone).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(GenerateJobResponse {
            job_id,
            status: "running".into(),
            target: target_name,
        }),
    ))
}

/// `GET /api/generate-tests/:job_id` — get generation result.
pub async fn get_job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<GenerateResultResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check completed jobs
    if let Some(result) = COMPLETED_JOBS.read().await.get(&job_id) {
        return Ok(Json(result.clone()));
    }

    // Check running jobs
    let jobs = state.jobs.read().await;
    if jobs.contains_key(&job_id) {
        return Ok(Json(GenerateResultResponse {
            job_id,
            status: "running".into(),
            result: None,
            error: None,
        }));
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse::not_found(&format!(
            "Job '{}' not found",
            job_id
        ))),
    ))
}

impl Clone for GenerateResultResponse {
    fn clone(&self) -> Self {
        Self {
            job_id: self.job_id.clone(),
            status: self.status.clone(),
            result: self.result.clone(),
            error: self.error.clone(),
        }
    }
}

/// Invoke the Python test generation backend via subprocess.
async fn invoke_python_gen(
    project_root: &std::path::Path,
    target: &str,
    framework: &str,
    edge_cases: bool,
    mocks: bool,
    max_tokens: usize,
) -> Result<GenerateResult, String> {
    let mut cmd = tokio::process::Command::new("python3");
    cmd.arg("-m")
        .arg("testforge_ai.cli_gen")
        .arg("--project")
        .arg(project_root.to_str().unwrap_or("."))
        .arg("--target")
        .arg(target)
        .arg("--framework")
        .arg(framework)
        .arg("--max-tokens")
        .arg(max_tokens.to_string());

    if edge_cases {
        cmd.arg("--edge-cases");
    }
    if mocks {
        cmd.arg("--mocks");
    }

    let python_path = project_root.join("python");
    cmd.env("PYTHONPATH", python_path.to_str().unwrap_or(""));

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to invoke Python: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Python generation failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("Invalid JSON from Python: {e}"))?;

    Ok(GenerateResult {
        source: result["source"].as_str().unwrap_or("").to_string(),
        file_name: result["file_name"]
            .as_str()
            .unwrap_or("test.py")
            .to_string(),
        target_symbol: result["target_symbol"]
            .as_str()
            .unwrap_or(target)
            .to_string(),
        test_count: result["test_count"].as_u64().unwrap_or(0) as usize,
        framework: framework.to_string(),
        warnings: result["warnings"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    })
}
