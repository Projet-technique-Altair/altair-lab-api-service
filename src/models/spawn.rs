use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct SpawnRequest {
    pub session_id: Uuid,
    pub lab_type: String, // Possibly replace with enum down the road
    pub template_path: String,
}

#[derive(Serialize)]
pub struct SpawnResponse {
    pub success: bool,
    pub data: SpawnResponseData,
}

#[derive(Serialize)]
pub struct SpawnResponseData {
    #[serde(rename = "data")]
    pub pod_name: String,
    pub webshell_url: String,
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

#[derive(Deserialize)]
pub struct StatusRequest {
    pub container_id: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
}
