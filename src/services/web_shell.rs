use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::AttachParams, Api};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::models::State;

const DEFAULT_NAMESPACE: &str = "default";
const BUFFER_SIZE: usize = 4096;

pub async fn handle_terminal(socket: WebSocket, pod_name: String, state: State) {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), DEFAULT_NAMESPACE);

    let attach_params = AttachParams {
        stdin: true,
        stdout: true,
        stderr: false,
        tty: true,
        ..Default::default()
    };

    let mut exec = match pods
        .exec(&pod_name, vec!["/bin/bash"], &attach_params)
        .await
    {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut stdin = exec.stdin().unwrap();
    let mut stdout = exec.stdout().unwrap();

    let (mut ws_tx, mut ws_rx) = socket.split();

    let to_pod = async {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Binary(data) => {
                    if stdin.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = stdin.shutdown().await;
    };

    let from_pod = async {
        let mut buf = [0u8; BUFFER_SIZE];

        while let Ok(n) = stdout.read(&mut buf).await {
            if n == 0 {
                break;
            }

            if ws_tx
                .send(Message::Binary(buf[..n].to_vec().into()))
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
