use axum::{extract::State, http::StatusCode, Json};

use crate::{
    models::{
        SpawnRequest, SpawnResponse, SpawnResponseData, StatusRequest, StatusResponse, StopRequest,
        StopResponse,
    },
    services::spawn,
};

pub async fn spawn_lab(
    State(state): State<crate::models::State>,
    Json(payload): Json<SpawnRequest>,
) -> Result<Json<SpawnResponse>, StatusCode> {
    let pod_name = spawn::spawn_lab(state, payload).await?;

    Ok(Json(SpawnResponse {
        success: true,
        data: SpawnResponseData {
            pod_name: pod_name.clone(),
            webshell_url: format!("ws://lab-api-service:8080/spawn/webshell/{}", pod_name),
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
    Json(payload): Json<StatusRequest>,
) -> Json<StatusResponse> {
    let status = spawn::status_lab(state, payload.container_id).await;

    Json(StatusResponse { status })
}
