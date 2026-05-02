/**
 * @file web_shell — HTTP route for WebSocket terminal access.
 *
 * @remarks
 * Exposes the endpoint used to establish a WebSocket connection
 * to a running lab Pod for interactive terminal access.
 *
 * Endpoint:
 *
 *  - `GET /spawn/webshell/:pod_name` → upgrade to WebSocket terminal session
 *
 * Key characteristics:
 *
 *  - Uses Axum WebSocket upgrade mechanism
 *  - Delegates connection handling to `services::web_shell`
 *  - Passes Pod identifier and application state to the handler
 *
 * This route acts as the entry point for terminal sessions,
 * enabling real-time interaction with lab containers.
 *
 * @packageDocumentation
 */
use axum::{
    extract::{ws::WebSocketUpgrade, Path, State},
    response::IntoResponse,
};

use crate::{models, services::web_shell};

pub async fn lab_terminal_ws(
    ws: WebSocketUpgrade,
    Path(pod_name): Path<String>,
    State(state): State<models::State>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| web_shell::handle_terminal(socket, pod_name, state))
}
