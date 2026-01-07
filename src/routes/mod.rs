pub mod health;
pub mod spawn;
pub mod web_shell;

use health::*;
use spawn::*;
use web_shell::lab_terminal_ws;

use axum::{
    routing::{get, post},
    Router,
};

pub fn init_routes() -> Router<crate::models::state::State> {
    Router::new()
        .route("/health", get(health))
        .route("/spawn", post(spawn_lab))
        .route("/spawn/stop", post(stop_lab))
        .route("/ws/labs/:pod_name", get(lab_terminal_ws))
}
