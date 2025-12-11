use axum::{Json, extract::State};
use crate::models::{SpawnRequest, SpawnResponse, StopRequest, StopResponse};

pub async fn spawn_lab(Json(_payload): Json<SpawnRequest>) -> Json<SpawnResponse> {
    Json(SpawnResponse {
        container_id: "mock-container".into(),
        webshell_url: "ws://localhost:8080/ws/mock".into(),
        status: "running".into(),
    })
}

pub async fn stop_lab(Json(_payload): Json<StopRequest>) -> Json<StopResponse> {
    Json(StopResponse {
        status: "stopped".into(),
    })
}
