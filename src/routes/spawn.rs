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
    let result = spawn::spawn_lab(state, payload).await?;

    // Get WebSocket base URL from environment variable
    let webshell_base_url =
        std::env::var("WEBSHELL_BASE_URL").unwrap_or_else(|_| "ws://localhost:8085".to_string());

    // For web labs, use the web URL as the webshell URL
    let webshell_url = if let Some(ref web_url) = result.web_url {
        web_url.clone()
    } else {
        format!(
            "{}/spawn/webshell/{}",
            webshell_base_url.trim_end_matches('/'),
            result.pod_name
        )
    };

    Ok(Json(SpawnResponse {
        success: true,
        data: SpawnResponseData {
            pod_name: result.pod_name,
            webshell_url,
            web_url: result.web_url,
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
