mod spawn;
mod web;
mod web_shell;

// Public for testing
pub mod health;

use axum::{
    routing::{any, get, post},
    Router,
};

use crate::models::State;

pub fn init_routes() -> Router<State> {
    let role = std::env::var("LAB_API_ROLE").unwrap_or_else(|_| "runtime-api".to_string());

    let router = Router::new().route("/health", get(health::health));

    // The same binary serves two roles. runtime-api keeps the existing spawn/webshell
    // endpoints, while web-proxy exposes only the internal /web routes.
    if role == "web-proxy" {
        router
            .route("/web/{container_id}", any(web::web_proxy_root_request))
            .route(
                "/web/{container_id}/{*path}",
                any(web::web_proxy_path_request),
            )
    } else {
        router
            .route("/spawn", post(spawn::spawn_lab))
            .route("/spawn/stop", post(spawn::stop_lab))
            .route("/spawn/status/{container_id}", get(spawn::status_lab))
            .route(
                "/web/session/{container_id}",
                post(web::bootstrap_web_session),
            )
            .route("/web/{container_id}", any(web::runtime_web_request))
            .route(
                "/web/{container_id}/{*path}",
                any(web::runtime_web_request),
            )
            .route(
                "/spawn/webshell/{pod_name}",
                get(web_shell::lab_terminal_ws),
            )
    }
}
