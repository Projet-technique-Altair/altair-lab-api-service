use std::{collections::BTreeMap, time::Duration};

use axum::http::{StatusCode, Uri};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::StreamExt;
use k8s_openapi::{
    api::core::v1::{
        Container, EmptyDirVolumeSource, LocalObjectReference, Pod, PodSpec, ResourceRequirements,
        Secret, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
    },
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
    ByteString,
};
use kube::{
    api::{DeleteParams, PostParams, WatchParams},
    Api,
};
use tokio::time::timeout;
use tracing::{error, info};

use crate::models::{SpawnRequest, State};

const GCP_SCOPE: &[&str] = &["https://www.googleapis.com/auth/cloud-platform"];
const DEFAULT_NAMESPACE: &str = "default";
const POD_TIMEOUT_SECS: u64 = 30;
const POD_DEADLINE_SECS: i64 = 7200;

pub async fn spawn_lab(state: State, payload: SpawnRequest) -> Result<String, StatusCode> {
    if !is_valid_lab_type(&payload.lab_type) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = &state.kube_client;
    let pods: Api<Pod> = Api::namespaced(client.clone(), DEFAULT_NAMESPACE);
    let secrets: Api<Secret> = Api::namespaced(client.clone(), DEFAULT_NAMESPACE);
    let services: Api<Service> = Api::namespaced(client.clone(), DEFAULT_NAMESPACE);

    let pod_name = format!("ctf-session-{}", payload.session_id);
    let secret_name = format!("gcr-secret-{}", payload.session_id);
    let web_service_name = format!("{}-web", pod_name);

    if state.local_mode {
        info!("Local mode enabled: skipping GCP image pull secret creation");
    } else {
        create_image_pull_secret(&state, &secrets, &secret_name, &payload.template_path).await?;
    }

    let pod = build_pod(&pod_name, &secret_name, &payload);
    pods.create(&PostParams::default(), &pod)
        .await
        .map_err(|e| {
            error!("Failed to create pod: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let pod_name = wait_for_pod_ready(&pods, &pod_name).await?;

    if payload.lab_delivery == "web" {
        create_web_service(&services, &web_service_name, &pod_name).await?;
    }

    Ok(pod_name)
}

pub async fn build_web_proxy_target(
    pod_name: &str,
    path: &str,
    request_uri: &Uri,
) -> Result<String, StatusCode> {
    let web_port = std::env::var("LAB_WEB_TARGET_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(3000);
    let service_name = format!("{}-web", pod_name);

    let query = request_uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();

    let upstream_path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path.trim_start_matches('/'))
    };

    Ok(format!(
        "http://{}.{}.svc.cluster.local:{}{}{}",
        service_name, DEFAULT_NAMESPACE, web_port, upstream_path, query
    ))
}

fn is_valid_lab_type(lab_type: &str) -> bool {
    let trimmed = lab_type.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 63
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

async fn create_image_pull_secret(
    state: &State,
    secrets: &Api<Secret>,
    secret_name: &str,
    template_path: &str,
) -> Result<(), StatusCode> {
    let provider = state.token_provider.as_ref().ok_or_else(|| {
        error!("Missing token provider in non-local mode");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let token = provider.token(GCP_SCOPE).await.map_err(|e| {
        error!("Failed to get GCP token: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let registry = template_path.split('/').next().unwrap_or("gcr.io");
    let auth_b64 = BASE64.encode(format!("oauth2accesstoken:{}", token.as_str()));

    let docker_config = serde_json::json!({
        "auths": { registry: { "auth": auth_b64 } }
    });

    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some(secret_name.to_string()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(BTreeMap::from([(
            ".dockerconfigjson".to_string(),
            ByteString(docker_config.to_string().into_bytes()),
        )])),
        ..Default::default()
    };

    let _ = secrets.delete(secret_name, &DeleteParams::default()).await;
    secrets
        .create(&PostParams::default(), &secret)
        .await
        .map_err(|e| {
            error!("Failed to create image pull secret: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

fn build_pod(pod_name: &str, secret_name: &str, payload: &SpawnRequest) -> Pod {
    let labels = BTreeMap::from([
        ("app".to_string(), "altair-lab".to_string()),
        ("session_id".to_string(), payload.session_id.to_string()),
        ("lab_type".to_string(), payload.lab_type.clone()),
        ("runtime_id".to_string(), pod_name.to_string()),
    ]);

    let limits = BTreeMap::from([
        ("memory".to_string(), Quantity("512Mi".into())),
        ("cpu".to_string(), Quantity("500m".into())),
    ]);

    let requests = BTreeMap::from([
        ("memory".to_string(), Quantity("256Mi".into())),
        ("cpu".to_string(), Quantity("250m".into())),
    ]);

    Pod {
        metadata: kube::core::ObjectMeta {
            name: Some(pod_name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            image_pull_secrets: Some(vec![LocalObjectReference {
                name: secret_name.to_string(),
            }]),
            containers: vec![Container {
                name: "lab-container".into(),
                image: Some(payload.template_path.clone()),
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
            active_deadline_seconds: Some(POD_DEADLINE_SECS),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn create_web_service(
    services: &Api<Service>,
    service_name: &str,
    pod_name: &str,
) -> Result<(), StatusCode> {
    let _ = services.delete(service_name, &DeleteParams::default()).await;

    let service = Service {
        metadata: kube::core::ObjectMeta {
            name: Some(service_name.to_string()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(BTreeMap::from([(
                "runtime_id".to_string(),
                pod_name.to_string(),
            )])),
            ports: Some(vec![ServicePort {
                port: 3000,
                target_port: Some(IntOrString::Int(3000)),
                ..Default::default()
            }]),
            type_: Some("ClusterIP".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    services
        .create(&PostParams::default(), &service)
        .await
        .map_err(|e| {
            error!("Failed to create web service {}: {:?}", service_name, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

async fn wait_for_pod_ready(pods: &Api<Pod>, pod_name: &str) -> Result<String, StatusCode> {
    let wp = WatchParams::default().fields(&format!("metadata.name={}", pod_name));
    let mut watcher = pods
        .watch(&wp, "0")
        .await
        .map_err(|e| {
            error!("Failed to watch pod: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .boxed();

    let result = timeout(Duration::from_secs(POD_TIMEOUT_SECS), async {
        while let Some(event) = watcher.next().await {
            let pod = match event {
                Ok(kube::api::WatchEvent::Added(p) | kube::api::WatchEvent::Modified(p)) => p,
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

    match result {
        Ok(Ok(())) => Ok(pod_name.to_string()),
        Ok(Err(status)) => Err(status),
        Err(_) => {
            error!("Timeout waiting for pod {} to become ready", pod_name);
            Err(StatusCode::REQUEST_TIMEOUT)
        }
    }
}

fn is_pod_ready(pod: &Pod) -> bool {
    let Some(status) = &pod.status else {
        return false;
    };

    let phase_running = status.phase.as_deref() == Some("Running");
    let containers_ready = status
        .container_statuses
        .as_ref()
        .is_some_and(|s| !s.is_empty() && s.iter().all(|cs| cs.ready));

    // Log container status for debugging
    if let Some(statuses) = &status.container_statuses {
        for cs in statuses {
            let Some(state) = &cs.state else { continue };

            if let Some(waiting) = &state.waiting {
                info!("Container {} waiting: {:?}", cs.name, waiting.reason);
            }
            if let Some(terminated) = &state.terminated {
                error!(
                    "Container {} terminated: reason={:?}, exit_code={}, message={:?}",
                    cs.name, terminated.reason, terminated.exit_code, terminated.message
                );
            }
        }
    }

    phase_running && containers_ready
}

fn is_pod_failed(pod: &Pod) -> bool {
    let Some(status) = &pod.status else {
        return false;
    };

    let phase_failed = status.phase.as_deref() == Some("Failed");

    let container_failed = status.container_statuses.as_ref().is_some_and(|statuses| {
        statuses.iter().any(|cs| {
            cs.state
                .as_ref()
                .and_then(|s| s.terminated.as_ref())
                .is_some_and(|t| t.exit_code != 0)
        })
    });

    phase_failed || container_failed
}

pub async fn delete_lab(state: State, pod_name: String) {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), DEFAULT_NAMESPACE);
    let services: Api<Service> = Api::namespaced(state.kube_client.clone(), DEFAULT_NAMESPACE);
    let web_service_name = format!("{}-web", pod_name);

    let _ = services
        .delete(&web_service_name, &DeleteParams::default())
        .await;
    pods.delete(&pod_name, &DeleteParams::default())
        .await
        .expect("Failed to delete pod");
}

pub async fn status_lab(state: State, pod_name: String) -> String {
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), DEFAULT_NAMESPACE);
    pods.get(&pod_name)
        .await
        .expect("Failed to get pod")
        .status
        .and_then(|s| s.phase)
        .unwrap_or_else(|| "Unknown".to_string())
}
