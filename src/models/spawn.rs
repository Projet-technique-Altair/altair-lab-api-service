use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct SpawnRequest {
    pub session_id: Uuid,
    pub lab_type: String,      // e.g. "ctf_terminal_guided"
    pub template_path: String, // e.g. "altair/lab-path-hijacking-guided:v1"
    pub lab_delivery: String,
    pub app_port: Option<i32>,
}

#[derive(Serialize)]
pub struct SpawnResponse {
    pub success: bool,
    pub data: SpawnResponseData,
}

#[derive(Serialize)]
pub struct SpawnResponseData {
    pub session_id: Uuid,
    pub container_id: String,
    pub status: String,
    pub runtime_kind: String,
    pub webshell_url: Option<String>,
    // app_url stays in the backend contract temporarily while LAB-WEB consumers
    // migrate to the bootstrap-tab flow; the frontend no longer relies on it.
    pub app_url: Option<String>,
}

#[derive(Deserialize)]
pub struct StopRequest {
    pub container_id: String,
}

#[derive(Serialize)]
pub struct StopResponse {
    pub status: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
}
