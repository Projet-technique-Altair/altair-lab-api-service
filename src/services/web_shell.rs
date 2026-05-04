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
 *  - Uses a TTY-enabled exec session with the container's current user
 *  - Does not force `su - student` or any fixed user
 *  - Sets a stable prompt displaying the real current user as user@altair:cwd
 *  - Binary WebSocket messages for efficient terminal I/O
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
use tracing::{error, info, warn};

use crate::models::State;

const DEFAULT_NAMESPACE: &str = "default";
const BUFFER_SIZE: usize = 4096;
const WEBSHELL_COMMAND: &str = r##"
USER_NAME="$(id -un 2>/dev/null || echo uid-$(id -u 2>/dev/null || echo unknown))"

if [ "$(id -u 2>/dev/null || echo 1)" = "0" ]; then
  PROMPT_CHAR="#"
else
  PROMPT_CHAR="$"
fi

export TERM="${TERM:-xterm-256color}"

if command -v bash >/dev/null 2>&1; then
  export PS1="${USER_NAME}@altair:\w${PROMPT_CHAR} "
  exec bash --noprofile --norc -i
fi

export PS1="${USER_NAME}@altair:\${PWD}${PROMPT_CHAR} "
exec sh -i
"##;

pub async fn handle_terminal(socket: WebSocket, pod_name: String, state: State) {
    let namespace = terminal_namespace();
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), &namespace);

    let attach_params = AttachParams {
        stdin: true,
        stdout: true,
        stderr: true,
        tty: true,
        ..Default::default()
    };

    info!(
        namespace = %namespace,
        pod_name = %pod_name,
        action = "webshell_exec",
        "opening web shell with current container user"
    );

    let mut exec = match pods
        .exec(
            &pod_name,
            vec!["/bin/sh", "-lc", WEBSHELL_COMMAND],
            &attach_params,
        )
        .await
    {
        Ok(e) => e,
        Err(error) => {
            error!(
                namespace = %namespace,
                pod_name = %pod_name,
                error = ?error,
                action = "webshell_exec",
                "failed to start web shell exec"
            );
            return;
        }
    };

    let Some(mut stdin) = exec.stdin() else {
        error!(
            namespace = %namespace,
            pod_name = %pod_name,
            action = "webshell_exec",
            "web shell exec did not provide stdin"
        );
        return;
    };

    let Some(mut stdout) = exec.stdout() else {
        error!(
            namespace = %namespace,
            pod_name = %pod_name,
            action = "webshell_exec",
            "web shell exec did not provide stdout"
        );
        return;
    };

    if let Some(mut stderr) = exec.stderr() {
        let stderr_namespace = namespace.clone();
        let stderr_pod_name = pod_name.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; BUFFER_SIZE];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let output = String::from_utf8_lossy(&buf[..n]);
                        warn!(
                            namespace = %stderr_namespace,
                            pod_name = %stderr_pod_name,
                            action = "webshell_stderr",
                            stderr = %output.trim_end(),
                            "web shell stderr"
                        );
                    }
                    Err(error) => {
                        warn!(
                            namespace = %stderr_namespace,
                            pod_name = %stderr_pod_name,
                            error = ?error,
                            action = "webshell_stderr",
                            "failed to read web shell stderr"
                        );
                        break;
                    }
                }
            }
        });
    }

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

fn terminal_namespace() -> String {
    std::env::var("LAB_TERMINAL_NAMESPACE").unwrap_or_else(|_| DEFAULT_NAMESPACE.to_string())
}

#[cfg(test)]
mod tests {
    use super::WEBSHELL_COMMAND;

    #[test]
    fn webshell_command_uses_current_user_prompt() {
        assert!(WEBSHELL_COMMAND.contains("id -un"));
        assert!(WEBSHELL_COMMAND.contains("uid-$(id -u"));
        assert!(WEBSHELL_COMMAND.contains("@altair"));
        assert!(WEBSHELL_COMMAND.contains("\\w"));
    }

    #[test]
    fn webshell_command_does_not_force_student() {
        assert!(!WEBSHELL_COMMAND.contains("su - student"));
        assert!(!WEBSHELL_COMMAND.contains("USER_NAME=\"student\""));
    }

    #[test]
    fn webshell_command_falls_back_to_sh() {
        assert!(WEBSHELL_COMMAND.contains("command -v bash"));
        assert!(WEBSHELL_COMMAND.contains("exec sh -i"));
    }
}
