/**
 * @file spawn — HTTP routes for lab runtime lifecycle.
 *
 * @remarks
 * Exposes endpoints to manage lab runtime instances,
 * including spawning, stopping, and querying status.
 *
 * Endpoints:
 *
 *  - `POST /spawn` → create a new lab runtime (Pod)
 *  - `POST /stop` → stop and delete a runtime
 *  - `GET /status/:container_id` → retrieve runtime status
 *
 * Key characteristics:
 *
 *  - Delegates orchestration logic to `services::spawn`
 *  - Supports both terminal and web runtimes
 *  - Dynamically builds access URLs (webshell or app)
 *  - Uses environment variables for base URLs
 *  - Returns structured responses for frontend consumption
 *
 * Features:
 *
 *  - Automatic runtime kind resolution (web vs terminal)
 *  - WebSocket endpoint generation for terminal sessions
 *  - HTTP endpoint generation for web labs
 *
 * This module acts as the public API layer for runtime management,
 * bridging client requests with Kubernetes-backed execution.
 *
 * @packageDocumentation
 */
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
    let session_id = payload.session_id;
    let runtime_kind = match payload.lab_delivery.as_str() {
        "web" => "web".to_string(),
        "terminal" => "terminal".to_string(),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let pod_name = spawn::spawn_lab(state, payload).await?;

    let webshell_base_url =
        std::env::var("WEBSHELL_BASE_URL").unwrap_or_else(|_| "ws://localhost:8085".to_string());
    let app_base_url =
        std::env::var("LAB_APP_BASE_URL").unwrap_or_else(|_| "http://localhost:8085".to_string());

    let (webshell_url, app_url) = if runtime_kind == "web" {
        // LAB-WEB still publishes app_url for backend compatibility, even though
        // the learner flow now opens the runtime through the bootstrap tab.
        (
            None,
            Some(format!(
                "{}/web/{}",
                app_base_url.trim_end_matches('/'),
                pod_name
            )),
        )
    } else {
        (
            Some(format!(
                "{}/spawn/webshell/{}",
                webshell_base_url.trim_end_matches('/'),
                pod_name
            )),
            None,
        )
    };

    Ok(Json(SpawnResponse {
        success: true,
        data: SpawnResponseData {
            session_id,
            container_id: pod_name,
            runtime_kind,
            webshell_url,
            app_url,
            status: "running".to_string(),
        },
    }))
}

pub async fn stop_lab(
    State(state): State<crate::models::State>,
    Json(payload): Json<StopRequest>,
) -> Json<StopResponse> {
    spawn::delete_lab(state, payload.container_id).await;

    Json(StopResponse {
        status: "stopped".to_string(),
    })
}

pub async fn status_lab(
    State(state): State<crate::models::State>,
    Path(container_id): Path<String>,
) -> Json<StatusResponse> {
    let status = spawn::status_lab(state, container_id).await;

    Json(StatusResponse { status })
}
