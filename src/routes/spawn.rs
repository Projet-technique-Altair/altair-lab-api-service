use crate::models::{SpawnRequest, SpawnResponse, StopRequest, StopResponse};
use axum::{extract::State, Json};
use k8s_openapi::api::core::v1::{Container, Pod, PodSpec};
use kube::api::PostParams;
use kube::Api;

pub async fn spawn_lab(
    State(state): State<crate::models::state::State>,
    Json(_payload): Json<SpawnRequest>,
) -> Json<SpawnResponse> {
    let pods: Api<Pod> = Api::namespaced(state.kube_client, "default");

    let pod = Pod {
        metadata: kube::core::ObjectMeta {
            name: Some("lab-session-123".into()),
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
        .expect("PULAMEA");
    Json(SpawnResponse {
        container_id: "mock-container".into(),
        webshell_url: "ws://localhost:8080/ws/mock".into(),
        status: "running".into(),
    })
}

pub async fn stop_lab(Json(_payload): Json<StopRequest>) -> Json<StopResponse> {
    Json(StopResponse {
        status: "stopped".into(),
    })
}
