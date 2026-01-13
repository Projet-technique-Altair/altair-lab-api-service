use axum::http::StatusCode;
use futures::StreamExt;
use k8s_openapi::api::core::v1::{
    Capabilities, Container, LocalObjectReference, Pod, PodSpec, ResourceRequirements,
    SecurityContext,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::WatchParams;
use kube::{
    api::{DeleteParams, PostParams},
    Api,
};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::time::timeout;

use crate::models::spawn::SpawnRequest;
use crate::models::state;

pub async fn spawn_lab(state: state::State, payload: SpawnRequest) -> Result<String, StatusCode> {
    if payload.lab_type != "ctf_terminal_guided" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");
    let pod_name = format!("ctf-session-{}", payload.session_id);

    let mut labels = BTreeMap::new();
    labels.insert("app".into(), "altair-lab".into());
    labels.insert("session_id".into(), payload.session_id.to_string());
    labels.insert("lab_type".into(), payload.lab_type.clone());

    let mut limits = BTreeMap::new();
    limits.insert("memory".into(), Quantity("512Mi".into()));
    limits.insert("cpu".into(), Quantity("500m".into()));

    let mut requests = BTreeMap::new();
    requests.insert("memory".into(), Quantity("256Mi".into()));
    requests.insert("cpu".into(), Quantity("250m".into()));

    let pod = Pod {
        metadata: kube::core::ObjectMeta {
            name: Some(pod_name.clone()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            image_pull_secrets: std::env::var("IMAGE_PULL_SECRET")
                .ok()
                .and_then(|name| Some(vec![LocalObjectReference { name }])),
            containers: vec![Container {
                name: "lab-container".into(),
                image: Some(payload.template_path.clone()),
                security_context: Some(SecurityContext {
                    run_as_user: Some(1000),
                    run_as_group: Some(1000),
                    allow_privilege_escalation: Some(false),
                    capabilities: Some(Capabilities {
                        drop: Some(vec!["ALL".into()]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                resources: Some(ResourceRequirements {
                    limits: Some(limits),
                    requests: Some(requests),
                    claims: None,
                }),
                ..Default::default()
            }],
            restart_policy: Some("Never".into()),
            active_deadline_seconds: Some(7200),
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

pub async fn delete_lab(state: state::State, pod_name: String) {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let dp = DeleteParams::default();
    pods.delete(&pod_name, &dp)
        .await
        .expect("Error: Deleting went wrong");
}

pub async fn status_lab(state: state::State, pod_name: String) -> String {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");
    pods.get(pod_name.as_str())
        .await
        .expect("Error: An error occurred while trying to get Pod by its name")
        .status
        .expect("Error: An error occurred while trying to get the status of a Pod")
        .phase
        .expect("Error: An error occurred while trying to get the status phase of the pod")
}
