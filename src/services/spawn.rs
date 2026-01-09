use axum::extract::State;
use axum::http::StatusCode;
use futures::StreamExt;
use k8s_openapi::api::core::v1::{Container, Pod, PodSpec};
use kube::api::WatchParams;
use kube::{
    api::{DeleteParams, PostParams},
    Api,
};
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

use crate::models::state;

pub async fn spawn_lab(State(state): State<state::State>) -> Result<String, StatusCode> {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let pod_name = Uuid::new_v4().to_string();

    let pod = Pod {
        metadata: kube::core::ObjectMeta {
            name: Some(pod_name.clone()),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "lab".into(),
                image: Some("debian:latest".into()),
                tty: Some(true),
                stdin: Some(true),
                ..Default::default()
            }],
            restart_policy: Some("Never".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    pods.create(&PostParams::default(), &pod)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let wp = WatchParams::default().fields(&format!("metadata.name={}", pod_name));

    let mut watcher = pods
        .watch(&wp, "0")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .boxed();

    let wait_result = timeout(Duration::from_secs(30), async {
        while let Some(event) = watcher.next().await {
            let pod = match event {
                Ok(kube::api::WatchEvent::Modified(p)) => p,
                _ => continue,
            };

            if is_pod_ready(&pod) {
                return Ok(());
            }
        }
        Err(())
    })
    .await;

    match wait_result {
        Ok(Ok(())) => Ok(pod_name),
        _ => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

fn is_pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        })
        .unwrap_or(false)
}

pub async fn delete_lab(State(state): State<state::State>, pod_name: String) {
    println!("{}", pod_name);
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let dp = DeleteParams::default();
    pods.delete(&pod_name, &dp)
        .await
        .expect("Error: Deleting went wrong");
}

pub async fn status_lab(State(state): State<state::State>, pod_name: String) -> String {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");
    let pod = pods.get(pod_name.as_str()).await.expect("Error: An error occurred while trying to get Pod by its name");
    pod.status.expect("Error: An error occurred while trying to get the status of a Pod").phase.expect("Error: An error occurred while trying to get the status phase of the pod")
}
