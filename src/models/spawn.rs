/**
 * @file spawn — runtime spawn and lifecycle models.
 *
 * @remarks
 * Defines the request and response structures used to manage
 * lab runtime instances (containers/sessions).
 *
 * Includes:
 *
 *  - Spawn request payload (`SpawnRequest`)
 *  - Spawn response structures (`SpawnResponse`, `SpawnResponseData`)
 *  - Stop request/response (`StopRequest`, `StopResponse`)
 *  - Status response (`StatusResponse`)
 *
 * Key characteristics:
 *
 *  - Identifies sessions and runtimes via UUIDs
 *  - Supports multiple lab types and delivery modes
 *  - Provides runtime metadata (container_id, runtime_kind)
 *  - Exposes access endpoints (webshell_url, optional app_url)
 *
 * This module represents the contract between the lab runtime API
 * and its consumers (gateway, frontend), handling lifecycle operations:
 * spawn → status → stop.
 *
 * @packageDocumentation
 */
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct SpawnRequest {
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub user_id: Option<Uuid>,
    pub lab_id: Option<Uuid>,
    pub lab_type: String,
    pub template_path: String,
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
