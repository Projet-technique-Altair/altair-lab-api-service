use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::StreamExt;
use k8s_openapi::api::core::v1::{
    Container, EmptyDirVolumeSource, LocalObjectReference, Pod, PodSpec, ResourceRequirements,
    Secret, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::ByteString;
use kube::api::WatchParams;
use kube::{
    api::{DeleteParams, PostParams},
    Api,
};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{error, info};

use crate::models::spawn::SpawnRequest;
use crate::models::state;

pub async fn spawn_lab(state: state::State, payload: SpawnRequest) -> Result<String, StatusCode> {
    if payload.lab_type != "ctf_terminal_guided" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");
    let secrets: Api<Secret> = Api::namespaced(state.kube_client.clone(), "default");
    let pod_name = format!("ctf-session-{}", payload.session_id);
    let secret_name = format!("gcr-secret-{}", payload.session_id);

    // Create image pull secret with GCP credentials
    let token = state
        .token_provider
        .token(&["https://www.googleapis.com/auth/cloud-platform"])
        .await
        .map_err(|e| {
            error!("Failed to get GCP token: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let access_token = token.as_str();

    // Extract registry hostname from template_path (e.g., "us-docker.pkg.dev/project/repo/image:tag")
    let registry = payload.template_path.split('/').next().unwrap_or("gcr.io");

    // Create docker config JSON for the registry
    let auth_string = format!("oauth2accesstoken:{}", access_token);
    let auth_b64 = BASE64.encode(auth_string.as_bytes());
    let docker_config = serde_json::json!({
        "auths": {
            registry: {
                "auth": auth_b64
            }
        }
    });

    let mut secret_data = BTreeMap::new();
    secret_data.insert(
        ".dockerconfigjson".to_string(),
        ByteString(docker_config.to_string().into_bytes()),
    );

    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some(secret_name.clone()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(secret_data),
        ..Default::default()
    };

    // Create or replace the secret
    let _ = secrets.delete(&secret_name, &DeleteParams::default()).await;
    secrets
        .create(&PostParams::default(), &secret)
        .await
        .map_err(|e| {
            error!("Failed to create image pull secret: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

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
            image_pull_secrets: Some(vec![LocalObjectReference {
                name: secret_name.clone(),
            }]),
            containers: vec![Container {
                name: "lab-container".into(),
                image: Some(payload.template_path.clone()),
                // Security context temporarily relaxed for debugging
                // TODO: Re-enable once container image is fixed
                // security_context: Some(SecurityContext {
                //     run_as_user: Some(1000),
                //     run_as_group: Some(1000),
                //     allow_privilege_escalation: Some(false),
                //     capabilities: Some(Capabilities {
                //         drop: Some(vec!["ALL".into()]),
                //         ..Default::default()
                //     }),
                //     ..Default::default()
                // }),
                resources: Some(ResourceRequirements {
                    limits: Some(limits),
                    requests: Some(requests),
                    claims: None,
                }),
                volume_mounts: Some(vec![VolumeMount {
                    name: "var-log".into(),
                    mount_path: "/var/log".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }],
            volumes: Some(vec![Volume {
                name: "var-log".into(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            }]),
            restart_policy: Some("Never".into()),
            active_deadline_seconds: Some(7200),
            ..Default::default()
        }),
        ..Default::default()
    };

    pods.create(&PostParams::default(), &pod)
        .await
        .map_err(|e| {
            error!("Failed to create pod: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let wp = WatchParams::default().fields(&format!("metadata.name={}", pod_name));
    let mut watcher = pods
        .watch(&wp, "0")
        .await
        .map_err(|e| {
            error!("Failed to watch pod: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .boxed();

    let wait_result = timeout(Duration::from_secs(30), async {
        while let Some(event) = watcher.next().await {
            let pod = match event {
                Ok(kube::api::WatchEvent::Added(p)) | Ok(kube::api::WatchEvent::Modified(p)) => p,
                _ => continue,
            };

            if is_pod_ready(&pod) {
                return Ok(());
            }

            if is_pod_failed(&pod) {
                error!("Pod {} failed to start", pod_name);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
        Err(StatusCode::REQUEST_TIMEOUT)
    })
    .await;

    match wait_result {
        Ok(Ok(())) => Ok(pod_name),
        Ok(Err(status)) => Err(status),
        Err(_) => {
            error!("Timeout waiting for pod {} to become ready", pod_name);
            Err(StatusCode::REQUEST_TIMEOUT)
        }
    }
}

fn is_pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .map(|status| {
            // Check if phase is Running
            let phase_running = status
                .phase
                .as_ref()
                .map(|p| p == "Running")
                .unwrap_or(false);

            // Check if all containers are ready
            let containers_ready = status
                .container_statuses
                .as_ref()
                .map(|statuses| !statuses.is_empty() && statuses.iter().all(|cs| cs.ready))
                .unwrap_or(false);

            // Log container status for debugging
            if let Some(statuses) = &status.container_statuses {
                for cs in statuses {
                    if let Some(state) = &cs.state {
                        if let Some(waiting) = &state.waiting {
                            info!("Container {} waiting: {:?}", cs.name, waiting.reason);
                        }
                        if let Some(terminated) = &state.terminated {
                            error!(
                                "Container {} terminated: reason={:?}, exit_code={}, message={:?}",
                                cs.name,
                                terminated.reason,
                                terminated.exit_code,
                                terminated.message
                            );
                        }
                    }
                }
            }

            phase_running && containers_ready
        })
        .unwrap_or(false)
}

/// Check if pod has failed (container crashed or terminated with error)
fn is_pod_failed(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .map(|status| {
            // Check if phase is Failed
            let phase_failed = status
                .phase
                .as_ref()
                .map(|p| p == "Failed")
                .unwrap_or(false);

            // Check if any container has terminated with non-zero exit code
            let container_failed = status
                .container_statuses
                .as_ref()
                .map(|statuses| {
                    statuses.iter().any(|cs| {
                        cs.state
                            .as_ref()
                            .map(|s| {
                                s.terminated
                                    .as_ref()
                                    .map(|t| t.exit_code != 0)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            phase_failed || container_failed
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
