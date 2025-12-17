use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SpawnRequest {
    pub lab_id: Option<String>,
}

#[derive(Serialize)]
pub struct SpawnResponse {
    pub container_id: String,
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
