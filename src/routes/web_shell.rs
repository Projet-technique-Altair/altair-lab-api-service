use axum::extract::ws::{WebSocketUpgrade, WebSocket, Message};
use axum::{extract::{Path, State}, response::IntoResponse};
use futures::{StreamExt, SinkExt};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::AttachParams;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn lab_terminal_ws(
    ws: WebSocketUpgrade,
    Path(pod_name): Path<String>,
    State(state): State<crate::models::state::State>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal(socket, pod_name, state))
}

// TODO: move this to services
async fn handle_terminal(
    socket: WebSocket,
    pod_name: String,
    state: crate::models::state::State,
) {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let ap = AttachParams {
        stdin: true,
        stdout: true,
        stderr: true,
        tty: false,
        ..Default::default()
    };

    let mut exec = match pods.exec(&pod_name, vec!["/bin/sh"], &ap).await {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut stdin = exec.stdin().unwrap();
    let mut stdout = exec.stdout().unwrap();

    let (mut ws_tx, mut ws_rx) = socket.split();

    let to_pod = async {
        while let Some(Ok(Message::Text(txt))) = ws_rx.next().await {
            let _ = stdin.write_all(txt.as_bytes()).await;
        }
    };

    let from_pod = async {
        let mut buf = [0u8; 1024];
        loop {
            let n = match stdout.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 { break; }
            let _ = ws_tx.send(Message::Binary(buf[..n].to_vec())).await;
        }
    };

    tokio::select! {
        _ = to_pod => {}
        _ = from_pod => {}
    }
}

