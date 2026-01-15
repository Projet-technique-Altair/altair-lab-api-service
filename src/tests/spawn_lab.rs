use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Container, ContainerState, ContainerStateRunning, ContainerStateTerminated,
    ContainerStateWaiting, ContainerStatus, EmptyDirVolumeSource, LocalObjectReference, Pod,
    PodSpec, PodStatus, ResourceRequirements, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use uuid::Uuid;

use crate::models::SpawnRequest;

// ============================================================================
// Helper functions for creating test data
// ============================================================================

fn create_test_spawn_request() -> SpawnRequest {
    SpawnRequest {
        session_id: Uuid::new_v4(),
        lab_type: "ctf_terminal_guided".to_string(),
        template_path: "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest".to_string(),
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
        ("lab_type".to_string(), payload.lab_type.clone()),
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
            active_deadline_seconds: Some(7200),
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
    assert_eq!(
        labels.get("lab_type"),
        Some(&"ctf_terminal_guided".to_string())
    );
    assert_eq!(
        labels.get("session_id"),
        Some(&payload.session_id.to_string())
    );
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
    assert_eq!(mounts[0].mount_path, "/var/log");
}

#[test]
fn test_build_pod_restart_policy() {
    let payload = create_test_spawn_request();
    let pod = build_pod("test-pod", "test-secret", &payload);

    let spec = pod.spec.unwrap();
    assert_eq!(spec.restart_policy, Some("Never".to_string()));
    assert_eq!(spec.active_deadline_seconds, Some(7200));
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
    let json = format!(
        r#"{{
            "session_id": "{}",
            "lab_type": "ctf_terminal_guided",
            "template_path": "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest"
        }}"#,
        session_id
    );

    let request: SpawnRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request.session_id, session_id);
    assert_eq!(request.lab_type, "ctf_terminal_guided");
    assert_eq!(
        request.template_path,
        "europe-west9-docker.pkg.dev/altair-isen/altair-labs/lab:latest"
    );
}

#[test]
fn test_spawn_request_invalid_uuid() {
    let json = r#"{
        "session_id": "invalid-uuid",
        "lab_type": "ctf_terminal_guided",
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
            "lab_type": "ctf_terminal_guided"
        }}"#,
        session_id
    );

    let result: Result<SpawnRequest, _> = serde_json::from_str(&json);
    assert!(result.is_err());
}

#[test]
fn test_spawn_response_serialize() {
    use crate::models::{SpawnResponse, SpawnResponseData};

    let response = SpawnResponse {
        success: true,
        data: SpawnResponseData {
            pod_name: "ctf-session-123".to_string(),
            webshell_url: "ws://lab-api-service:8080/spawn/webshell/ctf-session-123".to_string(),
            status: "RUNNING".to_string(),
        },
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""success":true"#));
    assert!(json.contains(r#""pod_name":"ctf-session-123""#));
    assert!(json.contains(r#""status":"RUNNING""#));
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
        status: "Stopped".to_string(),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""status":"Stopped""#));
}

#[test]
fn test_status_request_deserialize() {
    use crate::models::StatusRequest;

    let json = r#"{"container_id": "ctf-session-789"}"#;

    let request: StatusRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.container_id, "ctf-session-789");
}

#[test]
fn test_status_response_serialize() {
    use crate::models::StatusResponse;

    let response = StatusResponse {
        status: "Running".to_string(),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""status":"Running""#));
}

// ============================================================================
// Pod name generation tests
// ============================================================================

#[test]
fn test_pod_name_format() {
    let session_id = Uuid::new_v4();
    let pod_name = format!("ctf-session-{}", session_id);

    assert!(pod_name.starts_with("ctf-session-"));
    assert!(pod_name.contains(&session_id.to_string()));
}

#[test]
fn test_secret_name_format() {
    let session_id = Uuid::new_v4();
    let secret_name = format!("gcr-secret-{}", session_id);

    assert!(secret_name.starts_with("gcr-secret-"));
    assert!(secret_name.contains(&session_id.to_string()));
}

#[test]
fn test_webshell_url_format() {
    let session_id = Uuid::new_v4();
    let pod_name = format!("ctf-session-{}", session_id);
    let webshell_url = format!("ws://lab-api-service:8080/spawn/webshell/{}", pod_name);

    assert!(webshell_url.starts_with("ws://"));
    assert!(webshell_url.contains("/spawn/webshell/"));
    assert!(webshell_url.ends_with(&pod_name));
}
