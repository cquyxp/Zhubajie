//! HTTP API Server for controlling workers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::worker_boot::{Worker, WorkerRegistry, WorkerTaskReceipt};

/// API Server state
#[derive(Clone)]
pub struct ApiServerState {
    registry: WorkerRegistry,
}

/// Create worker request
#[derive(Debug, Deserialize)]
pub struct CreateWorkerRequest {
    pub cwd: String,
    pub trusted_roots: Option<Vec<String>>,
    pub auto_recover_prompt_misdelivery: Option<bool>,
}

/// Send prompt request
#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    pub task_receipt: Option<WorkerTaskReceipt>,
}

/// Generic API response
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.to_string()),
        }
    }
}

/// Create a new API server router
pub fn create_router(registry: WorkerRegistry) -> Router {
    let state = ApiServerState { registry };

    Router::new()
        .route("/", get(|| async { "Zhubajie API Server" }))
        .route("/workers", post(create_worker))
        .route("/workers", get(list_workers))
        .route("/workers/:id", get(get_worker))
        .route("/workers/:id/prompt", post(send_prompt))
        .route("/workers/:id/resolve-trust", post(resolve_trust))
        .route("/workers/:id/observe", post(observe_screen))
        .with_state(state)
}

async fn create_worker(
    State(state): State<ApiServerState>,
    Json(request): Json<CreateWorkerRequest>,
) -> impl IntoResponse {
    let trusted_roots = request.trusted_roots.unwrap_or_default();
    let auto_recover = request.auto_recover_prompt_misdelivery.unwrap_or(false);

    let worker = state
        .registry
        .create(&request.cwd, &trusted_roots, auto_recover);

    (StatusCode::CREATED, Json(ApiResponse::success(worker)))
}

async fn list_workers(State(state): State<ApiServerState>) -> impl IntoResponse {
    // WorkerRegistry doesn't expose a way to list all workers, so we'll return empty for now
    // This is a placeholder until WorkerRegistry adds list functionality
    let workers: Vec<Worker> = Vec::new();
    Json(ApiResponse::success(workers))
}

async fn get_worker(
    State(state): State<ApiServerState>,
    Path(worker_id): Path<String>,
) -> impl IntoResponse {
    match state.registry.get(&worker_id) {
        Some(worker) => (StatusCode::OK, Json(ApiResponse::success(Some(worker)))),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some("Worker not found".to_string()),
            }),
        ),
    }
}

async fn send_prompt(
    State(state): State<ApiServerState>,
    Path(worker_id): Path<String>,
    Json(request): Json<SendPromptRequest>,
) -> impl IntoResponse {
    match state
        .registry
        .send_prompt(&worker_id, Some(&request.prompt), request.task_receipt)
    {
        Ok(worker) => (StatusCode::OK, Json(ApiResponse::success(Some(worker)))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

async fn resolve_trust(
    State(state): State<ApiServerState>,
    Path(worker_id): Path<String>,
) -> impl IntoResponse {
    match state.registry.resolve_trust(&worker_id) {
        Ok(worker) => (StatusCode::OK, Json(ApiResponse::success(Some(worker)))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct ObserveScreenRequest {
    pub screen_text: String,
}

async fn observe_screen(
    State(state): State<ApiServerState>,
    Path(worker_id): Path<String>,
    Json(request): Json<ObserveScreenRequest>,
) -> impl IntoResponse {
    match state.registry.observe(&worker_id, &request.screen_text) {
        Ok(worker) => (StatusCode::OK, Json(ApiResponse::success(Some(worker)))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

/// Start the API server on the given port
pub async fn start_server(registry: WorkerRegistry, port: u16) -> Result<(), std::io::Error> {
    let app = create_router(registry);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
