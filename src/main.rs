use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

// -------------------- DATA STRUCTURES --------------------

#[derive(Deserialize)]
struct SpawnRequest {
    lab_id: Option<String>,
}

#[derive(Serialize)]
struct SpawnResponse {
    container_id: String,
    webshell_url: String,
    status: String,
}

#[derive(Deserialize)]
struct StopRequest {
    container_id: String,
}

#[derive(Serialize)]
struct StopResponse {
    status: String,
}

// -------------------- MAIN --------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/spawn", post(spawn_lab))
        .route("/spawn/stop", post(stop_lab));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8085));
    println!("Server running on http://localhost:8085");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// -------------------- ROUTES --------------------

async fn health() -> &'static str {
    "OK"
}

async fn spawn_lab(Json(_payload): Json<SpawnRequest>) -> Json<SpawnResponse> {
    Json(SpawnResponse {
        container_id: "mock-container".to_string(),
        webshell_url: "ws://localhost:8080/ws/mock".to_string(),
        status: "running".to_string(),
    })
}

async fn stop_lab(Json(_payload): Json<StopRequest>) -> Json<StopResponse> {
    Json(StopResponse {
        status: "stopped".to_string(),
    })
}
