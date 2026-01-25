use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::{
    models::{
        SpawnRequest, SpawnResponse, SpawnResponseData, StatusResponse, StopRequest, StopResponse,
    },
    services::spawn,
};

pub async fn spawn_lab(
    State(state): State<crate::models::State>,
    Json(payload): Json<SpawnRequest>,
) -> Result<Json<SpawnResponse>, StatusCode> {
    let pod_name = spawn::spawn_lab(state, payload).await?;

    // Get WebSocket base URL from environment variable
    let webshell_base_url =
        std::env::var("WEBSHELL_BASE_URL").unwrap_or_else(|_| "ws://localhost:8085".to_string());
    let webshell_url = format!(
        "{}/spawn/webshell/{}",
        webshell_base_url.trim_end_matches('/'),
        pod_name
    );

    Ok(Json(SpawnResponse {
        success: true,
        data: SpawnResponseData {
            pod_name: pod_name.clone(),
            webshell_url,
            status: "RUNNING".to_string(),
        },
    }))
}

pub async fn stop_lab(
    State(state): State<crate::models::State>,
    Json(payload): Json<StopRequest>,
) -> Json<StopResponse> {
    spawn::delete_lab(state, payload.container_id).await;

    Json(StopResponse {
        status: "Stopped".to_string(),
    })
}

pub async fn status_lab(
    State(state): State<crate::models::State>,
    Path(container_id): Path<String>,
) -> Json<StatusResponse> {
    let status = spawn::status_lab(state, container_id).await;

    Json(StatusResponse { status })
}
