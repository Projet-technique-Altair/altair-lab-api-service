/**
 * @file web_shell — WebSocket terminal bridge for lab runtimes.
 *
 * @remarks
 * Provides a real-time interactive shell by bridging a WebSocket connection
 * with a Kubernetes Pod exec session.
 *
 * Responsibilities:
 *
 *  - Attach to a running Pod using Kubernetes exec
 *  - Forward WebSocket input to the Pod's stdin
 *  - Stream Pod stdout back to the WebSocket client
 *  - Handle bidirectional communication asynchronously
 *
 * Key characteristics:
 *
 *  - Uses TTY-enabled exec session (`/bin/bash` as student user)
 *  - Binary WebSocket messages for efficient data transfer
 *  - Non-blocking I/O with async streams
 *  - Graceful shutdown on connection close or errors
 *
 * This module enables interactive terminal access for lab sessions,
 * acting as a bridge between the frontend WebSocket client
 * and the containerized lab environment.
 *
 * @packageDocumentation
 */
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
        .exec(
            &pod_name,
            vec!["/bin/bash", "-lc", "exec su - student"],
            &attach_params,
        )
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
