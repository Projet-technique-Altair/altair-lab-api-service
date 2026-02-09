use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Clone)]
pub struct SpawnRequest {
    pub session_id: Uuid,
    pub lab_type: String,      // e.g. "ctf_terminal_guided"
    pub template_path: String, // e.g. "altair/lab-path-hijacking-guided:v1"
}

#[derive(Serialize)]
pub struct SpawnResponse {
    pub success: bool,
    pub data: SpawnResponseData,
}

#[derive(Serialize)]
pub struct SpawnResponseData {
    pub pod_name: String,
    pub webshell_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
    pub status: String,
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
