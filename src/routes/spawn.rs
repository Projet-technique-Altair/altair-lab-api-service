use crate::models::{SpawnRequest, SpawnResponse, StopRequest, StopResponse};
use axum::{extract::State, Json};
use k8s_openapi::api::core::v1::{Container, Pod, PodSpec};
use kube::api::AttachParams;
use kube::api::PostParams;
use kube::Api;
use uuid::Uuid;

pub async fn spawn_lab(
    State(state): State<crate::models::state::State>,
    Json(_payload): Json<SpawnRequest>,
) -> Json<SpawnResponse> {
    let pods: Api<Pod> = Api::namespaced(state.kube_client, "default");

    let pod_name = format!("lab-session-{}", Uuid::new_v4());

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
        .expect("failed to create pod");

    // TODO: wait for pod to be Running

    Json(SpawnResponse {
        container_id: pod_name.clone(),
        webshell_url: format!("ws://localhost:8080/ws/labs/{pod_name}"),
        status: "running".into(),
    })

}

pub async fn stop_lab(Json(_payload): Json<StopRequest>) -> Json<StopResponse> {
    Json(StopResponse {
        status: "stopped".into(),
    })
}
