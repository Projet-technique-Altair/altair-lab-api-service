/**
 * @file spawn_lab — lab runtime orchestration tests.
 *
 * @remarks
 * Tests the Kubernetes resource definitions, validation rules,
 * runtime status helpers, and request/response models used by lab spawning.
 *
 * Test coverage:
 *
 *  - Pod metadata, labels, container configuration, resources, and volumes
 *  - Web Service generation for web-based labs
 *  - Namespace selection by delivery mode
 *  - Spawn payload validation rules
 *  - Pod readiness and failure detection
 *  - Model serialization and deserialization
 *  - Runtime naming conventions for Pods, secrets, and WebSocket URLs
 *
 * Key characteristics:
 *
 *  - Focuses on deterministic logic without requiring a live Kubernetes cluster
 *  - Mirrors service-side resource construction for regression safety
 *  - Validates both terminal and web runtime flows
 *
 * These tests help ensure that lab runtime objects remain compatible
 * with the orchestration service and frontend/API expectations.
 *
 * @packageDocumentation
 */
use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Container, ContainerState, ContainerStateRunning, ContainerStateTerminated,
    ContainerStateWaiting, ContainerStatus, EmptyDirVolumeSource, LocalObjectReference, Pod,
    PodSpec, PodStatus, ResourceRequirements, Service, ServicePort, ServiceSpec, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use uuid::Uuid;

use crate::models::SpawnRequest;

// Constants matching the service implementation
const POD_DEADLINE_SECS: i64 = 7200;
const DEFAULT_NAMESPACE: &str = "default";
const WEB_NAMESPACE: &str = "labs-web";
const WEB_SERVICE_PORT: i32 = 80;

// ============================================================================
// Helper functions for creating test data
// ============================================================================

fn create_test_spawn_request() -> SpawnRequest {
    SpawnRequest {
        session_id: Uuid::new_v4(),
        runtime_id: Uuid::new_v4(),
        user_id: Some(Uuid::new_v4()),
        lab_id: Some(Uuid::new_v4()),
        lab_type: "guided_terminal".to_string(),
        template_path: "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest".to_string(),
        lab_delivery: "terminal".to_string(),
        app_port: None,
    }
}

fn create_pod_with_status(
    phase: Option<&str>,
    container_statuses: Option<Vec<ContainerStatus>>,
) -> Pod {
    Pod {
        status: Some(PodStatus {
            phase: phase.map(String::from),
            container_statuses,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn create_running_container_status(ready: bool) -> ContainerStatus {
    ContainerStatus {
        name: "lab-container".to_string(),
        ready,
        state: Some(ContainerState {
            running: Some(ContainerStateRunning::default()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn create_waiting_container_status(reason: &str) -> ContainerStatus {
    ContainerStatus {
        name: "lab-container".to_string(),
        ready: false,
        state: Some(ContainerState {
            waiting: Some(ContainerStateWaiting {
                reason: Some(reason.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn create_terminated_container_status(exit_code: i32, reason: &str) -> ContainerStatus {
    ContainerStatus {
        name: "lab-container".to_string(),
        ready: false,
        state: Some(ContainerState {
            terminated: Some(ContainerStateTerminated {
                exit_code,
                reason: Some(reason.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

// ============================================================================
// build_pod tests
// ============================================================================

fn build_pod(pod_name: &str, secret_name: &str, payload: &SpawnRequest) -> Pod {
    let labels = BTreeMap::from([
        ("app".to_string(), "altair-lab".to_string()),
        ("session_id".to_string(), payload.session_id.to_string()),
        ("runtime_id".to_string(), payload.runtime_id.to_string()),
        ("lab_type".to_string(), payload.lab_type.clone()),
        ("runtime_kind".to_string(), payload.lab_delivery.clone()),
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

fn is_valid_app_port(app_port: i32) -> bool {
    (1..=65535).contains(&app_port)
}

fn is_valid_spawn_payload(payload: &SpawnRequest) -> bool {
    match payload.lab_delivery.as_str() {
        "web" => payload.app_port.is_some_and(is_valid_app_port),
        "terminal" => payload.app_port.is_none() || payload.app_port.is_some_and(is_valid_app_port),
        _ => false,
    }
}

fn namespace_for_delivery(lab_delivery: &str) -> &'static str {
    if lab_delivery == "web" {
        WEB_NAMESPACE
    } else {
        DEFAULT_NAMESPACE
    }
}

fn build_web_service_name(pod_name: &str) -> String {
    format!("{pod_name}-web")
}

fn build_web_service(pod_name: &str, payload: &SpawnRequest) -> Service {
    Service {
        metadata: kube::core::ObjectMeta {
            name: Some(build_web_service_name(pod_name)),
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

#[test]
fn test_build_pod_metadata() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    assert_eq!(pod.metadata.name, Some("test-pod".to_string()));

    let labels = pod.metadata.labels.unwrap();
    assert_eq!(labels.get("app"), Some(&"altair-lab".to_string()));
    assert_eq!(labels.get("lab_type"), Some(&"guided_terminal".to_string()));
    assert_eq!(
        labels.get("session_id"),
        Some(&payload.session_id.to_string())
    );
    assert_eq!(
        labels.get("runtime_id"),
        Some(&payload.runtime_id.to_string())
    );
    assert_eq!(labels.get("runtime_kind"), Some(&"terminal".to_string()));
}

#[test]
fn test_build_pod_image_pull_secrets() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "my-secret", &payload);

    let spec = pod.spec.unwrap();
    let pull_secrets = spec.image_pull_secrets.unwrap();

    assert_eq!(pull_secrets.len(), 1);
    assert_eq!(pull_secrets[0].name, "my-secret");
}

#[test]
fn test_build_pod_container_configuration() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    let spec = pod.spec.unwrap();
    assert_eq!(spec.containers.len(), 1);

    let container = &spec.containers[0];
    assert_eq!(container.name, "lab-container");
    assert_eq!(
        container.image,
        Some("europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest".to_string())
    );
}

#[test]
fn test_build_pod_resource_limits() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    let container = &pod.spec.unwrap().containers[0];
    let resources = container.resources.as_ref().unwrap();

    let limits = resources.limits.as_ref().unwrap();
    assert_eq!(limits.get("memory"), Some(&Quantity("512Mi".into())));
    assert_eq!(limits.get("cpu"), Some(&Quantity("500m".into())));

    let requests = resources.requests.as_ref().unwrap();
    assert_eq!(requests.get("memory"), Some(&Quantity("256Mi".into())));
    assert_eq!(requests.get("cpu"), Some(&Quantity("250m".into())));
}

#[test]
fn test_build_pod_volumes() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    let spec = pod.spec.unwrap();

    // Check volumes
    let volumes = spec.volumes.unwrap();
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].name, "var-log");
    assert!(volumes[0].empty_dir.is_some());

    // Check volume mounts
    let container = &spec.containers[0];
    let mounts = container.volume_mounts.as_ref().unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].name, "var-log");
    assert_eq!(mounts[0].mount_path, "/var/log/altair");
}

#[test]
fn test_build_pod_restart_policy() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    let spec = pod.spec.unwrap();
    assert_eq!(spec.restart_policy, Some("Never".to_string()));
    assert_eq!(spec.active_deadline_seconds, Some(POD_DEADLINE_SECS));
}

#[test]
fn test_build_web_service_configuration() {
    let mut payload = create_test_spawn_request();
    payload.lab_delivery = "web".to_string();
    payload.app_port = Some(3000);

    let service = build_web_service("ctf-session-123", &payload);
    assert_eq!(
        service.metadata.name,
        Some("ctf-session-123-web".to_string())
    );

    let spec = service.spec.unwrap();
    assert_eq!(spec.type_, Some("ClusterIP".to_string()));
    assert_eq!(
        spec.selector.unwrap().get("runtime_kind"),
        Some(&"web".to_string())
    );

    let port = &spec.ports.unwrap()[0];
    assert_eq!(port.port, WEB_SERVICE_PORT);
    assert_eq!(port.target_port, Some(IntOrString::Int(3000)));
}

#[test]
fn test_namespace_for_delivery() {
    assert_eq!(namespace_for_delivery("terminal"), DEFAULT_NAMESPACE);
    assert_eq!(namespace_for_delivery("web"), WEB_NAMESPACE);
}

#[test]
fn test_is_valid_spawn_payload_requires_port_for_web() {
    let mut payload = create_test_spawn_request();
    payload.lab_delivery = "web".to_string();
    payload.app_port = None;

    assert!(!is_valid_spawn_payload(&payload));
}

#[test]
fn test_is_valid_spawn_payload_accepts_web_with_valid_port() {
    let mut payload = create_test_spawn_request();
    payload.lab_delivery = "web".to_string();
    payload.app_port = Some(8080);

    assert!(is_valid_spawn_payload(&payload));
}

// ============================================================================
// is_pod_ready tests
// ============================================================================

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

#[test]
fn test_is_pod_ready_running_and_ready() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_running_container_status(true)]),
    );

    assert!(is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_running_but_not_ready() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_running_container_status(false)]),
    );

    assert!(!is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_pending_phase() {
    let pod = create_pod_with_status(Some("Pending"), None);
    assert!(!is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_no_status() {
    let pod = Pod::default();
    assert!(!is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_container_waiting() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_waiting_container_status("ContainerCreating")]),
    );

    assert!(!is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_empty_container_statuses() {
    let pod = create_pod_with_status(Some("Running"), Some(vec![]));
    assert!(!is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_multiple_containers_all_ready() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![
            create_running_container_status(true),
            create_running_container_status(true),
        ]),
    );

    assert!(is_pod_ready(&pod));
}

#[test]
fn test_is_pod_ready_multiple_containers_one_not_ready() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![
            create_running_container_status(true),
            create_running_container_status(false),
        ]),
    );

    assert!(!is_pod_ready(&pod));
}

// ============================================================================
// is_pod_failed tests
// ============================================================================

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

#[test]
fn test_is_pod_failed_phase_failed() {
    let pod = create_pod_with_status(Some("Failed"), None);
    assert!(is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_container_terminated_with_error() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_terminated_container_status(1, "Error")]),
    );

    assert!(is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_container_terminated_with_oom() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_terminated_container_status(137, "OOMKilled")]),
    );

    assert!(is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_container_terminated_successfully() {
    let pod = create_pod_with_status(
        Some("Succeeded"),
        Some(vec![create_terminated_container_status(0, "Completed")]),
    );

    assert!(!is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_no_status() {
    let pod = Pod::default();
    assert!(!is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_running_healthy() {
    let pod = create_pod_with_status(
        Some("Running"),
        Some(vec![create_running_container_status(true)]),
    );

    assert!(!is_pod_failed(&pod));
}

#[test]
fn test_is_pod_failed_pending() {
    let pod = create_pod_with_status(Some("Pending"), None);
    assert!(!is_pod_failed(&pod));
}

// ============================================================================
// Model serialization/deserialization tests
// ============================================================================

#[test]
fn test_spawn_request_deserialize() {
    let session_id = Uuid::new_v4();
    let runtime_id = Uuid::new_v4();
    let json = format!(
        r#"{{
            "session_id": "{}",
            "runtime_id": "{}",
            "lab_type": "guided_terminal",
            "template_path": "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest",
            "lab_delivery": "terminal",
            "app_port": null
        }}"#,
        session_id, runtime_id
    );

    let request: SpawnRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request.session_id, session_id);
    assert_eq!(request.runtime_id, runtime_id);
    assert_eq!(request.lab_type, "guided_terminal");
    assert_eq!(request.lab_delivery, "terminal");
    assert_eq!(request.app_port, None);
    assert_eq!(
        request.template_path,
        "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest"
    );
}

#[test]
fn test_spawn_request_deserialize_web_with_app_port() {
    let session_id = Uuid::new_v4();
    let runtime_id = Uuid::new_v4();
    let json = format!(
        r#"{{
            "session_id": "{}",
            "runtime_id": "{}",
            "lab_type": "guided_web",
            "template_path": "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest",
            "lab_delivery": "web",
            "app_port": 3000
        }}"#,
        session_id, runtime_id
    );

    let request: SpawnRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request.session_id, session_id);
    assert_eq!(request.runtime_id, runtime_id);
    assert_eq!(request.lab_delivery, "web");
    assert_eq!(request.app_port, Some(3000));
}

#[test]
fn test_spawn_request_invalid_uuid() {
    let json = r#"{
        "session_id": "invalid-uuid",
        "lab_type": "guided_terminal",
        "template_path": "some/path"
    }"#;

    let result: Result<SpawnRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn test_spawn_request_missing_field() {
    let session_id = Uuid::new_v4();
    let json = format!(
        r#"{{
            "session_id": "{}",
            "lab_type": "guided_terminal"
        }}"#,
        session_id
    );

    let result: Result<SpawnRequest, _> = serde_json::from_str(&json);
    assert!(result.is_err());
}

#[test]
fn test_spawn_response_serialize() {
    use crate::models::{SpawnResponse, SpawnResponseData};
    use uuid::Uuid;

    let response = SpawnResponse {
        success: true,
        data: SpawnResponseData {
            session_id: Uuid::nil(),
            container_id: "ctf-session-123".to_string(),
            status: "running".to_string(),
            runtime_kind: "terminal".to_string(),
            webshell_url: Some(
                "ws://lab-api-service:8080/spawn/webshell/ctf-session-123".to_string(),
            ),
            app_url: None,
        },
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""success":true"#));
    assert!(json.contains(r#""container_id":"ctf-session-123""#));
    assert!(json.contains(r#""status":"running""#));
    assert!(json.contains(r#""runtime_kind":"terminal""#));
    assert!(json.contains(r#""webshell_url""#));
}

#[test]
fn test_stop_request_deserialize() {
    use crate::models::StopRequest;

    let json = r#"{"container_id": "ctf-session-456"}"#;

    let request: StopRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.container_id, "ctf-session-456");
}

#[test]
fn test_stop_response_serialize() {
    use crate::models::StopResponse;

    let response = StopResponse {
        status: "stopped".to_string(),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""status":"stopped""#));
}

#[test]
fn test_status_response_serialize() {
    use crate::models::StatusResponse;

    let response = StatusResponse {
        status: "running".to_string(),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""status":"running""#));
}

fn is_valid_lab_type(lab_type: &str) -> bool {
    let trimmed = lab_type.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 63
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

#[test]
fn test_is_valid_lab_type_accepts_legacy_and_new_values() {
    assert!(is_valid_lab_type("ctf_terminal_guided"));
    assert!(is_valid_lab_type("ctf_web_non_guided"));
    assert!(is_valid_lab_type("guided_terminal"));
    assert!(is_valid_lab_type("web"));
}

#[test]
fn test_is_valid_lab_type_rejects_empty_or_invalid_values() {
    assert!(!is_valid_lab_type(""));
    assert!(!is_valid_lab_type("   "));
    assert!(!is_valid_lab_type("guided terminal"));
    assert!(!is_valid_lab_type("guided/terminal"));
}

// ============================================================================
// Pod name generation tests
// ============================================================================

#[test]
fn test_pod_name_format() {
    let runtime_id = Uuid::new_v4();
    let pod_name = format!("ctf-runtime-{}", runtime_id);

    assert!(pod_name.starts_with("ctf-runtime-"));
    assert!(pod_name.contains(&runtime_id.to_string()));
}

#[test]
fn test_secret_name_format() {
    let runtime_id = Uuid::new_v4();
    let secret_name = format!("gcr-secret-{}", runtime_id);

    assert!(secret_name.starts_with("gcr-secret-"));
    assert!(secret_name.contains(&runtime_id.to_string()));
}

#[test]
fn test_webshell_url_format() {
    let runtime_id = Uuid::new_v4();
    let pod_name = format!("ctf-runtime-{}", runtime_id);
    let webshell_url = format!("ws://lab-api-service:8080/spawn/webshell/{}", pod_name);

    assert!(webshell_url.starts_with("ws://"));
    assert!(webshell_url.contains("/spawn/webshell/"));
    assert!(webshell_url.ends_with(&pod_name));
}
