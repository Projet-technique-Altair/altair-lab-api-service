use axum::{
    extract::{ws::WebSocketUpgrade, Path, State},
    response::IntoResponse,
};

use crate::models::state;
use crate::services::web_shell::handle_terminal;

pub async fn lab_terminal_ws(
    ws: WebSocketUpgrade,
    Path(pod_name): Path<String>,
    State(state): State<state::State>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal(socket, pod_name, state))
}
