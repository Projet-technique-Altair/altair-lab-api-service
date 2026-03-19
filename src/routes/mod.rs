mod spawn;
mod web_shell;

// Public for testing
pub mod health;

use axum::{
    routing::{get, post},
    Router,
};

use crate::models::State;

pub fn init_routes() -> Router<State> {
    Router::new()
        .route("/health", get(health::health))
        .route("/spawn", post(spawn::spawn_lab))
        .route("/spawn/stop", post(spawn::stop_lab))
        .route("/spawn/status/{container_id}", get(spawn::status_lab))
        .route("/app/{container_id}", get(spawn::proxy_web_root))
        .route("/app/{container_id}/{*path}", get(spawn::proxy_web_path))
        .route(
            "/spawn/webshell/{pod_name}",
            get(web_shell::lab_terminal_ws),
        )
}
