/**
 * @file routes — application route registration.
 *
 * @remarks
 * Defines and registers all HTTP routes exposed by the Lab API service,
 * mapping endpoints to their corresponding handlers.
 *
 * Registered routes:
 *
 *  - `GET /health` → service health check
 *  - `POST /spawn` → create a new lab runtime (Pod)
 *  - `POST /spawn/stop` → stop and delete a runtime
 *  - `GET /spawn/status/{container_id}` → retrieve runtime status
 *  - `POST /web/open-session/{session_id}` → open a secured web lab session
 *  - `GET /spawn/webshell/{pod_name}` → WebSocket terminal access
 *
 * Key characteristics:
 *
 *  - Centralized routing configuration
 *  - Uses shared application state (`State`)
 *  - Connects HTTP layer to route handlers
 *
 * This module acts as the entry point for all API endpoints,
 * assembling the router used by the application server.
 *
 * @packageDocumentation
 */

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
