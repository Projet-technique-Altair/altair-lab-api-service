mod spawn;
mod web;
mod web_shell;

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
        .route(
            "/web/open-session/{session_id}",
            post(web::open_web_session),
        )
        .route(
            "/spawn/webshell/{pod_name}",
            get(web_shell::lab_terminal_ws),
        )
}
