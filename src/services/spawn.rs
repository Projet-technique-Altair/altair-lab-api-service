/**
 * @file spawn — Kubernetes runtime orchestration service.
 *
 * @remarks
 * Implements the core logic for spawning, stopping, and checking
 * lab runtime instances in Kubernetes.
 *
 * Responsibilities:
 *
 *  - Validate spawn requests and lab delivery modes
 *  - Create Kubernetes Pods for lab runtimes
 *  - Create image pull secrets for private registries
 *  - Create ClusterIP Services for web-based labs
 *  - Wait for Pods to become ready
 *  - Delete runtime resources when sessions stop
 *  - Retrieve runtime status from Kubernetes
 *
 * Key characteristics:
 *
 *  - Supports terminal and web lab delivery modes
 *  - Splits runtimes across dedicated namespaces
 *  - Supports local mode by skipping GCP image pull secret creation
 *  - Enforces resource limits and runtime deadlines
 *  - Uses Kubernetes labels to scope sessions and runtime instances
 *
 * This service is responsible for translating lab session requests
 * into concrete Kubernetes resources used to run isolated lab environments.
 *
 * @packageDocumentation
 */
use std::{collections::BTreeMap, time::Duration};

use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::StreamExt;
use k8s_openapi::{
    api::core::v1::{
        Container, EmptyDirVolumeSource, EnvVar, LocalObjectReference, Pod, PodSpec,
        ResourceRequirements, Secret, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
    apimachinery::pkg::util::intstr::IntOrString,
    ByteString,
};
use kube::{
    api::{DeleteParams, PostParams, WatchParams},
    Api,
};
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::models::{SpawnRequest, State};

const GCP_SCOPE: &[&str] = &["https://www.googleapis.com/auth/cloud-platform"];
const DEFAULT_NAMESPACE: &str = "default";
const WEB_NAMESPACE: &str = "labs-web";
const POD_TIMEOUT_SECS: u64 = 30;
const POD_DEADLINE_SECS: i64 = 7200;
const WEB_SERVICE_PORT: i32 = 80;
const LAB_CONTAINER_NAME: &str = "lab-container";
const TERMINAL_KEEPALIVE_SCRIPT: &str = r#"
if [ -x /opt/altair/startup.sh ]; then
  /opt/altair/startup.sh || true
fi

echo "[altair] runtime user: $(id 2>/dev/null || true)" >&2

trap 'exit 0' TERM INT
while true; do
  sleep 3600
done
"#;

#[derive(Debug, Clone, Default)]
struct PodDiagnostics {
    phase: Option<String>,
    normalized_status: String,
    ready: bool,
    container_state: Option<String>,
    reason: Option<String>,
    message: Option<String>,
    exit_code: Option<i32>,
}

pub async fn spawn_lab(state: State, payload: SpawnRequest) -> Result<String, StatusCode> {
    if !is_valid_lab_type(&payload.lab_type) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !is_valid_spawn_payload(&payload) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = &state.kube_client;
    let namespace = namespace_for_delivery(&payload.lab_delivery);
    let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);
    let secrets: Api<Secret> = Api::namespaced(client.clone(), &namespace);
    let services: Api<Service> = Api::namespaced(client.clone(), &namespace);

    // Runtime ids scope infra names so one session can cycle through multiple Pods.
    let pod_name = format!("ctf-runtime-{}", payload.runtime_id);
    let secret_name = format!("gcr-secret-{}", payload.runtime_id);
    let use_image_pull_secret = !state.local_mode;

    info!(
        session_id = %payload.session_id,
        runtime_id = %payload.runtime_id,
        namespace = %namespace,
        pod_name = %pod_name,
        lab_delivery = %payload.lab_delivery,
        lab_type = %payload.lab_type,
        image = %payload.template_path,
        app_port = ?payload.app_port,
        action = "create_pod",
        "spawning lab runtime"
    );

    if state.local_mode {
        info!(
            namespace = %namespace,
            pod_name = %pod_name,
            "Local mode enabled: skipping GCP image pull secret creation"
        );
    } else {
        create_image_pull_secret(&state, &secrets, &secret_name, &payload.template_path).await?;
    }

    let pod = build_pod(&pod_name, &secret_name, &payload, use_image_pull_secret);
    pods.create(&PostParams::default(), &pod)
        .await
        .map_err(|e| {
            error!(
                namespace = %namespace,
                pod_name = %pod_name,
                lab_delivery = %payload.lab_delivery,
                error = ?e,
                action = "create_pod",
                "failed to create lab pod"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Web labs need a stable in-cluster Service before the future web proxy can
    // forward requests to the Pod without depending on an ephemeral Pod IP.
    if payload.lab_delivery == "web" {
        create_web_session_service(&services, &pod_name, &payload, &namespace).await?;
    }

    wait_for_pod_ready(&pods, &pod_name, &payload, &namespace).await
}

fn is_valid_lab_type(lab_type: &str) -> bool {
    let trimmed = lab_type.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 63
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn is_valid_spawn_payload(payload: &SpawnRequest) -> bool {
    match payload.lab_delivery.as_str() {
        // Web sessions need an explicit container port so lab-api can create the
        // matching Kubernetes Service with a deterministic targetPort.
        "web" => payload.app_port.is_some_and(is_valid_app_port),
        "terminal" => payload.app_port.is_none() || payload.app_port.is_some_and(is_valid_app_port),
        _ => false,
    }
}

fn is_valid_app_port(app_port: i32) -> bool {
    (1..=65535).contains(&app_port)
}

fn namespace_for_delivery(lab_delivery: &str) -> String {
    if lab_delivery == "web" {
        std::env::var("LAB_WEB_NAMESPACE").unwrap_or_else(|_| WEB_NAMESPACE.to_string())
    } else {
        std::env::var("LAB_TERMINAL_NAMESPACE").unwrap_or_else(|_| DEFAULT_NAMESPACE.to_string())
    }
}

fn build_web_service_name(pod_name: &str) -> String {
    format!("{pod_name}-web")
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
        error!(error = ?e, "Failed to get GCP token");
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
            error!(secret_name = %secret_name, error = ?e, "Failed to create image pull secret");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

fn build_pod(
    pod_name: &str,
    secret_name: &str,
    payload: &SpawnRequest,
    use_image_pull_secret: bool,
) -> Pod {
    let mut labels = BTreeMap::from([
        ("app".to_string(), "altair-lab".to_string()),
        ("session_id".to_string(), payload.session_id.to_string()),
        ("runtime_id".to_string(), payload.runtime_id.to_string()),
        ("lab_type".to_string(), payload.lab_type.clone()),
        // This keeps the future web session Service scoped to web runtimes only.
        ("runtime_kind".to_string(), payload.lab_delivery.clone()),
    ]);

    if let Some(user_id) = payload.user_id {
        labels.insert("user_id".to_string(), user_id.to_string());
    }
    if let Some(lab_id) = payload.lab_id {
        labels.insert("lab_id".to_string(), lab_id.to_string());
    }

    let limits = BTreeMap::from([
        ("memory".to_string(), Quantity("512Mi".into())),
        ("cpu".to_string(), Quantity("500m".into())),
    ]);

    let requests = BTreeMap::from([
        ("memory".to_string(), Quantity("256Mi".into())),
        ("cpu".to_string(), Quantity("250m".into())),
    ]);

    let is_terminal = payload.lab_delivery == "terminal";

    Pod {
        metadata: kube::core::ObjectMeta {
            name: Some(pod_name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            image_pull_secrets: if use_image_pull_secret {
                Some(vec![LocalObjectReference {
                    name: secret_name.to_string(),
                }])
            } else {
                None
            },
            containers: vec![Container {
                name: LAB_CONTAINER_NAME.into(),
                image: Some(payload.template_path.clone()),
                image_pull_policy: Some("Always".into()),
                command: is_terminal.then(|| vec!["/bin/sh".to_string(), "-lc".to_string()]),
                args: is_terminal.then(|| vec![TERMINAL_KEEPALIVE_SCRIPT.to_string()]),
                env: Some(build_session_flag_env(payload)),
                resources: Some(ResourceRequirements {
                    limits: Some(limits),
                    requests: Some(requests),
                    claims: None,
                }),
                volume_mounts: Some(vec![VolumeMount {
                    name: "var-log".into(),
                    mount_path: "/var/log/altair".into(),
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

fn build_session_flag_env(payload: &SpawnRequest) -> Vec<EnvVar> {
    let mut env = Vec::new();

    if let Some(flags) = payload.session_flags.as_object() {
        for (step_number, flag) in flags {
            let Some(flag) = flag.as_str() else {
                continue;
            };
            env.push(EnvVar {
                name: format!("ALTAIR_FLAG_STEP_{}", step_number),
                value: Some(flag.to_string()),
                ..Default::default()
            });
        }
    }

    env
}

fn build_web_service(pod_name: &str, payload: &SpawnRequest) -> Service {
    let service_name = build_web_service_name(pod_name);

    Service {
        metadata: kube::core::ObjectMeta {
            name: Some(service_name),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(BTreeMap::from([
                ("app".to_string(), "altair-lab".to_string()),
                ("runtime_id".to_string(), payload.runtime_id.to_string()),
                ("runtime_kind".to_string(), "web".to_string()),
            ])),
            ports: Some(vec![ServicePort {
                port: WEB_SERVICE_PORT,
                target_port: payload.app_port.map(IntOrString::Int),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn create_web_session_service(
    services: &Api<Service>,
    pod_name: &str,
    payload: &SpawnRequest,
    namespace: &str,
) -> Result<(), StatusCode> {
    let service_name = build_web_service_name(pod_name);
    let service = build_web_service(pod_name, payload);

    let _ = services
        .delete(&service_name, &DeleteParams::default())
        .await;
    services
        .create(&PostParams::default(), &service)
        .await
        .map_err(|e| {
            error!(
                namespace = %namespace,
                pod_name = %pod_name,
                service_name = %service_name,
                error = ?e,
                action = "create_web_service",
                "failed to create web session service"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

async fn wait_for_pod_ready(
    pods: &Api<Pod>,
    pod_name: &str,
    payload: &SpawnRequest,
    namespace: &str,
) -> Result<String, StatusCode> {
    let wp = WatchParams::default().fields(&format!("metadata.name={}", pod_name));
    let mut watcher = pods
        .watch(&wp, "0")
        .await
        .map_err(|e| {
            error!(
                namespace = %namespace,
                pod_name = %pod_name,
                error = ?e,
                action = "wait_ready",
                "failed to watch pod"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .boxed();

    let result = timeout(Duration::from_secs(POD_TIMEOUT_SECS), async {
        while let Some(event) = watcher.next().await {
            let pod = match event {
                Ok(kube::api::WatchEvent::Added(p) | kube::api::WatchEvent::Modified(p)) => p,
                Ok(_) => continue,
                Err(e) => {
                    error!(
                        namespace = %namespace,
                        pod_name = %pod_name,
                        error = ?e,
                        action = "wait_ready",
                        "pod watch event failed"
                    );
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            let diagnostics = pod_diagnostics(&pod);

            if diagnostics.ready {
                info!(
                    session_id = %payload.session_id,
                    runtime_id = %payload.runtime_id,
                    namespace = %namespace,
                    pod_name = %pod_name,
                    phase = ?diagnostics.phase,
                    status = %diagnostics.normalized_status,
                    action = "wait_ready",
                    "lab pod is ready"
                );
                return Ok(());
            }

            if is_fatal_waiting_reason(&diagnostics) {
                log_pod_diagnostics(
                    "lab pod is waiting with a fatal reason before becoming ready",
                    payload,
                    namespace,
                    pod_name,
                    &diagnostics,
                );
                return Err(StatusCode::BAD_GATEWAY);
            }

            if is_pod_failed(&pod) {
                log_pod_diagnostics(
                    "lab pod failed before becoming ready",
                    payload,
                    namespace,
                    pod_name,
                    &diagnostics,
                );
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            if is_pod_completed(&pod) {
                log_pod_diagnostics(
                    "lab pod completed before becoming ready",
                    payload,
                    namespace,
                    pod_name,
                    &diagnostics,
                );
                return Err(StatusCode::CONFLICT);
            }
        }
        Err(StatusCode::REQUEST_TIMEOUT)
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(pod_name.to_string()),
        Ok(Err(status)) => Err(status),
        Err(_) => {
            error!(
                session_id = %payload.session_id,
                runtime_id = %payload.runtime_id,
                namespace = %namespace,
                pod_name = %pod_name,
                lab_delivery = %payload.lab_delivery,
                action = "wait_ready",
                timeout_secs = POD_TIMEOUT_SECS,
                "timeout waiting for pod to become ready"
            );
            Err(StatusCode::REQUEST_TIMEOUT)
        }
    }
}

fn pod_diagnostics(pod: &Pod) -> PodDiagnostics {
    let Some(status) = &pod.status else {
        return PodDiagnostics {
            normalized_status: "unknown".to_string(),
            ..Default::default()
        };
    };

    let ready = is_pod_ready(pod);
    let mut diagnostics = PodDiagnostics {
        phase: status.phase.clone(),
        normalized_status: normalize_pod_phase(status.phase.as_deref()).to_string(),
        ready,
        ..Default::default()
    };

    let Some(container_status) = status.container_statuses.as_ref().and_then(|statuses| {
        statuses
            .iter()
            .find(|cs| cs.name == LAB_CONTAINER_NAME)
            .or_else(|| statuses.first())
    }) else {
        return diagnostics;
    };

    let Some(container_state) = &container_status.state else {
        return diagnostics;
    };

    if let Some(waiting) = &container_state.waiting {
        diagnostics.container_state = Some("waiting".to_string());
        diagnostics.reason = waiting.reason.clone();
        diagnostics.message = waiting.message.clone();
    } else if let Some(running) = &container_state.running {
        diagnostics.container_state = Some("running".to_string());
        diagnostics.message = running
            .started_at
            .as_ref()
            .map(|_| "container started".to_string());
    } else if let Some(terminated) = &container_state.terminated {
        diagnostics.container_state = Some("terminated".to_string());
        diagnostics.reason = terminated.reason.clone();
        diagnostics.message = terminated.message.clone();
        diagnostics.exit_code = Some(terminated.exit_code);
    }

    diagnostics
}

fn normalize_pod_phase(phase: Option<&str>) -> &'static str {
    match phase {
        Some("Pending") => "starting",
        Some("Running") => "running",
        Some("Succeeded") => "completed",
        Some("Failed") => "failed",
        _ => "unknown",
    }
}

fn is_fatal_waiting_reason(diagnostics: &PodDiagnostics) -> bool {
    matches!(
        diagnostics.reason.as_deref(),
        Some("ErrImagePull")
            | Some("ImagePullBackOff")
            | Some("CreateContainerConfigError")
            | Some("RunContainerError")
            | Some("InvalidImageName")
            | Some("CrashLoopBackOff")
    )
}

fn log_pod_diagnostics(
    message: &str,
    payload: &SpawnRequest,
    namespace: &str,
    pod_name: &str,
    diagnostics: &PodDiagnostics,
) {
    error!(
        session_id = %payload.session_id,
        runtime_id = %payload.runtime_id,
        lab_delivery = %payload.lab_delivery,
        lab_type = %payload.lab_type,
        namespace = %namespace,
        pod_name = %pod_name,
        image = %payload.template_path,
        phase = ?diagnostics.phase,
        status = %diagnostics.normalized_status,
        ready = diagnostics.ready,
        container_state = ?diagnostics.container_state,
        reason = ?diagnostics.reason,
        k8s_message = ?diagnostics.message,
        exit_code = ?diagnostics.exit_code,
        action = "wait_ready",
        "{}",
        message
    );
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

fn is_pod_completed(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|status| status.phase.as_deref())
        == Some("Succeeded")
}

pub async fn delete_lab(state: State, pod_name: String) {
    // Stop requests only carry the container_id, so deletion checks both runtime
    // namespaces and always cleans the derived web Service name as well.
    let terminal_namespace = namespace_for_delivery("terminal");
    let web_namespace = namespace_for_delivery("web");
    let terminal_pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), &terminal_namespace);
    let web_pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), &web_namespace);
    let web_services: Api<Service> = Api::namespaced(state.kube_client.clone(), &web_namespace);

    let _ = delete_pod_if_exists(&terminal_pods, &pod_name, &terminal_namespace).await;
    let _ = delete_pod_if_exists(&web_pods, &pod_name, &web_namespace).await;
    let _ = delete_service_if_exists(
        &web_services,
        &build_web_service_name(&pod_name),
        &web_namespace,
    )
    .await;
}

pub async fn status_lab(state: State, pod_name: String) -> String {
    // Status checks follow the same namespace split as stop: terminal in the
    // terminal namespace, web in the web namespace.
    let terminal_namespace = namespace_for_delivery("terminal");
    let web_namespace = namespace_for_delivery("web");
    let terminal_pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), &terminal_namespace);
    let web_pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), &web_namespace);

    if let Some(status) = get_pod_status(&terminal_pods, &pod_name, &terminal_namespace).await {
        return status;
    }

    get_pod_status(&web_pods, &pod_name, &web_namespace)
        .await
        .unwrap_or_else(|| "unknown".to_string())
}

async fn delete_pod_if_exists(pods: &Api<Pod>, pod_name: &str, namespace: &str) -> bool {
    match pods.delete(pod_name, &DeleteParams::default()).await {
        Ok(_) => true,
        Err(kube::Error::Api(api_error)) if api_error.code == 404 => false,
        Err(error) => {
            error!(namespace = %namespace, pod_name = %pod_name, error = ?error, "Failed to delete pod");
            false
        }
    }
}

async fn delete_service_if_exists(
    services: &Api<Service>,
    service_name: &str,
    namespace: &str,
) -> bool {
    match services
        .delete(service_name, &DeleteParams::default())
        .await
    {
        Ok(_) => true,
        Err(kube::Error::Api(api_error)) if api_error.code == 404 => false,
        Err(error) => {
            error!(namespace = %namespace, service_name = %service_name, error = ?error, "Failed to delete service");
            false
        }
    }
}

async fn get_pod_status(pods: &Api<Pod>, pod_name: &str, namespace: &str) -> Option<String> {
    match pods.get(pod_name).await {
        Ok(pod) => {
            let diagnostics = pod_diagnostics(&pod);
            if diagnostics.container_state == Some("terminated".to_string())
                || diagnostics.reason.is_some()
                || diagnostics.normalized_status == "failed"
                || diagnostics.normalized_status == "completed"
            {
                warn!(
                    namespace = %namespace,
                    pod_name = %pod_name,
                    phase = ?diagnostics.phase,
                    status = %diagnostics.normalized_status,
                    ready = diagnostics.ready,
                    container_state = ?diagnostics.container_state,
                    reason = ?diagnostics.reason,
                    k8s_message = ?diagnostics.message,
                    exit_code = ?diagnostics.exit_code,
                    action = "status_check",
                    "lab pod status contains diagnostic details"
                );
            }
            Some(diagnostics.normalized_status)
        }
        Err(kube::Error::Api(api_error)) if api_error.code == 404 => None,
        Err(error) => {
            error!(namespace = %namespace, pod_name = %pod_name, error = ?error, "Failed to get pod status");
            Some("unknown".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_pod, normalize_pod_phase, TERMINAL_KEEPALIVE_SCRIPT};
    use crate::models::SpawnRequest;
    use uuid::Uuid;

    fn terminal_spawn_request() -> SpawnRequest {
        SpawnRequest {
            session_id: Uuid::new_v4(),
            runtime_id: Uuid::new_v4(),
            user_id: None,
            lab_id: None,
            lab_type: "guided_terminal".to_string(),
            template_path: "example.test/lab:latest".to_string(),
            lab_delivery: "terminal".to_string(),
            app_port: None,
            session_flags: serde_json::json!({}),
        }
    }

    #[test]
    fn terminal_pod_uses_altair_keepalive_command() {
        let payload = terminal_spawn_request();
        let pod = build_pod("test-pod", "test-secret", &payload, true);
        let container = &pod.spec.unwrap().containers[0];

        assert_eq!(
            container.command,
            Some(vec!["/bin/sh".to_string(), "-lc".to_string()])
        );
        assert_eq!(
            container.args,
            Some(vec![TERMINAL_KEEPALIVE_SCRIPT.to_string()])
        );
    }

    #[test]
    fn web_pod_keeps_image_cmd_or_entrypoint() {
        let mut payload = terminal_spawn_request();
        payload.lab_delivery = "web".to_string();
        payload.app_port = Some(3000);
        let pod = build_pod("test-pod", "test-secret", &payload, true);
        let container = &pod.spec.unwrap().containers[0];

        assert!(container.command.is_none());
        assert!(container.args.is_none());
    }

    #[test]
    fn local_mode_does_not_reference_image_pull_secret() {
        let payload = terminal_spawn_request();
        let pod = build_pod("test-pod", "test-secret", &payload, false);

        assert!(pod.spec.unwrap().image_pull_secrets.is_none());
    }

    #[test]
    fn pod_phase_is_normalized_for_public_status() {
        assert_eq!(normalize_pod_phase(Some("Pending")), "starting");
        assert_eq!(normalize_pod_phase(Some("Running")), "running");
        assert_eq!(normalize_pod_phase(Some("Succeeded")), "completed");
        assert_eq!(normalize_pod_phase(Some("Failed")), "failed");
        assert_eq!(normalize_pod_phase(None), "unknown");
    }
}
