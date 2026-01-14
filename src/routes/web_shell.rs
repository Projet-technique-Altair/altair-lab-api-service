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
