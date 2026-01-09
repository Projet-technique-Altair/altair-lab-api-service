use crate::models::{
    SpawnRequest, SpawnResponse, StatusRequest, StatusResponse, StopRequest, StopResponse,
};
use crate::services::spawn;

use axum::{extract::State, Json};

pub async fn spawn_lab(
    State(state): State<crate::models::state::State>,
    Json(_payload): Json<SpawnRequest>,
) -> Json<SpawnResponse> {
    // !!! For now just deploying a debian image
    // TODO: Next step in the implementation - get the lab id and get the container from the registry
    let pod_name = spawn::spawn_lab(State(state)).await.expect("An error has occurred while spawning the pod");

    Json(SpawnResponse {
        container_id: pod_name.clone(),
        webshell_url: format!("ws://localhost:8085/spawn/webshell/{pod_name}"),
        status: "Running".into(),
    })
}

pub async fn stop_lab(
    State(state): State<crate::models::state::State>,
    Json(payload): Json<StopRequest>,
) -> Json<StopResponse> {
    // TODO: Implement error handling
    spawn::delete_lab(State(state), payload.container_id).await;
    Json(StopResponse {
        status: "Stopped".into(),
    })
}

pub async fn status_lab(
    State(state): State<crate::models::state::State>,
    Json(payload): Json<StatusRequest>,
) -> Json<StatusResponse> {
    Json(StatusResponse {
        status: spawn::status_lab(State(state), payload.container_id).await,
    })
}
