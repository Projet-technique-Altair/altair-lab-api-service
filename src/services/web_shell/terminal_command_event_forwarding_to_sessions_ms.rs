//! Batch and forward captured terminal command events to sessions-ms.

use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::warn;
use uuid::Uuid;

const EVENT_BATCH_SIZE: usize = 10;
const EVENT_FLUSH_SECS: u64 = 2;
const EVENT_QUEUE_SIZE: usize = 256;

#[derive(Clone)]
pub(super) struct TerminalCommandEventForwarder {
    tx: mpsc::Sender<TerminalCommandEvent>,
}

#[derive(Clone)]
struct TerminalEventContext {
    session_id: Uuid,
    runtime_id: Uuid,
    user_id: Uuid,
    lab_id: Uuid,
}

#[derive(Clone, Serialize)]
struct TerminalCommandEvent {
    event_id: Uuid,
    occurred_at: DateTime<Utc>,
    command_redacted: String,
    exit_status: Option<i32>,
}

#[derive(Serialize)]
struct TerminalEventsPayload {
    session_id: Uuid,
    runtime_id: Uuid,
    user_id: Uuid,
    lab_id: Uuid,
    events: Vec<TerminalCommandEvent>,
}

pub(super) async fn start_terminal_command_event_forwarder(
    pods: &Api<Pod>,
    pod_name: &str,
) -> Option<TerminalCommandEventForwarder> {
    let context = load_terminal_event_context(pods, pod_name).await?;
    let (tx, rx) = mpsc::channel(EVENT_QUEUE_SIZE);
    tokio::spawn(forward_terminal_events(context, rx));

    Some(TerminalCommandEventForwarder { tx })
}

impl TerminalCommandEventForwarder {
    pub(super) fn send_redacted_command(&self, command_redacted: String) {
        if command_redacted.is_empty() {
            return;
        }

        if self
            .tx
            .try_send(TerminalCommandEvent {
                event_id: Uuid::new_v4(),
                occurred_at: Utc::now(),
                command_redacted,
                exit_status: None,
            })
            .is_err()
        {
            warn!("Dropped terminal command event because the analytics queue is full");
        }
    }
}

async fn load_terminal_event_context(
    pods: &Api<Pod>,
    pod_name: &str,
) -> Option<TerminalEventContext> {
    let pod = pods.get(pod_name).await.ok()?;
    let labels = pod.metadata.labels?;

    Some(TerminalEventContext {
        session_id: labels
            .get("session_id")
            .and_then(|v| Uuid::parse_str(v).ok())?,
        runtime_id: labels
            .get("runtime_id")
            .and_then(|v| Uuid::parse_str(v).ok())?,
        user_id: labels
            .get("user_id")
            .and_then(|v| Uuid::parse_str(v).ok())?,
        lab_id: labels.get("lab_id").and_then(|v| Uuid::parse_str(v).ok())?,
    })
}

async fn forward_terminal_events(
    context: TerminalEventContext,
    mut rx: mpsc::Receiver<TerminalCommandEvent>,
) {
    let mut events = Vec::new();
    let mut ticker = interval(Duration::from_secs(EVENT_FLUSH_SECS));

    loop {
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        events.push(event);
                        if events.len() >= EVENT_BATCH_SIZE {
                            flush_terminal_events(&context, &mut events).await;
                        }
                    }
                    None => break,
                }
            }
            _ = ticker.tick() => {
                if !events.is_empty() {
                    flush_terminal_events(&context, &mut events).await;
                }
            }
        }
    }

    if !events.is_empty() {
        flush_terminal_events(&context, &mut events).await;
    }
}

async fn flush_terminal_events(
    context: &TerminalEventContext,
    events: &mut Vec<TerminalCommandEvent>,
) {
    let batch = std::mem::take(events);
    let sessions_url =
        std::env::var("SESSIONS_MS_URL").unwrap_or_else(|_| "http://localhost:3003".to_string());
    let url = format!(
        "{}/internal/terminal-events",
        sessions_url.trim_end_matches('/')
    );

    let payload = TerminalEventsPayload {
        session_id: context.session_id,
        runtime_id: context.runtime_id,
        user_id: context.user_id,
        lab_id: context.lab_id,
        events: batch,
    };

    if let Err(error) = reqwest::Client::new().post(url).json(&payload).send().await {
        warn!(
            "Failed to forward terminal events to sessions-ms: {}",
            error
        );
    }
}
