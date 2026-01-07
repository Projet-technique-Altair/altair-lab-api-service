use axum::extract::ws::{Message, WebSocket};

use futures::{SinkExt, StreamExt};

use kube::{
    api::AttachParams,
    Api,
};
use k8s_openapi::api::core::v1::Pod;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::models::state;

pub async fn handle_terminal(
    socket: WebSocket,
    pod_name: String,
    state: state::State,
) {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let ap = AttachParams {
        stdin: true,
        stdout: true,
        stderr: false,
        tty: true,
        ..Default::default()
    };

    let mut exec = match pods.exec(&pod_name, vec!["/bin/bash"], &ap).await {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut stdin = exec.stdin().unwrap();
    let mut stdout = exec.stdout().unwrap();

    let (mut ws_tx, mut ws_rx) = socket.split();

    let to_pod = async {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                // ✅ BEST CASE: raw bytes (xterm.js, websocat -b)
                Message::Binary(data) => {
                    if stdin.write_all(&data).await.is_err() {
                        break;
                    }
                }

                // ⚠️ Fallback: text input (websocat without -b)
                Message::Text(text) => {
                    let mut data = text.into_bytes();
                    data.push(b'\n'); // 🔑 REQUIRED for /bin/sh
                    if stdin.write_all(&data).await.is_err() {
                        break;
                    }
                }

                Message::Close(_) => break,
                _ => {}
            }
        }

        // Only close stdin when WS is gone
        let _ = stdin.shutdown().await;
    };

    let from_pod = async {
        let mut buf = [0u8; 4096];

        loop {
            let n = match stdout.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => break,
            };

            if n == 0 {
                break;
            }

            if ws_tx
                .send(Message::Binary(buf[..n].to_vec()))
                .await
                .is_err()
            {
                break;
            }
        }
    };

    tokio::select! {
        _ = to_pod => {}
        _ = from_pod => {}
    }
}
