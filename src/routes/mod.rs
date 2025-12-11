pub mod health;
pub mod spawn;

pub use health::*;
pub use spawn::*;

use axum::Router;
use axum::routing::{get, post};

pub fn init_routes() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/spawn", post(spawn_lab))
        .route("/spawn/stop", post(stop_lab))
}
