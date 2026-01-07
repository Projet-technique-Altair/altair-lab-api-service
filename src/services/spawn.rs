use axum::extract::State;

use k8s_openapi::api::core::v1::{Container, Pod, PodSpec};
use kube::{
    api::{DeleteParams, PostParams},
    Api,
};
use uuid::Uuid;

use crate::models::state;

pub async fn spawn_lab(State(state): State<state::State>) -> String {
    let pods: Api<Pod> = Api::namespaced(state.kube_client, "default");

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
        .expect("failed to create pod");
    pod_name
}

pub async fn delete_lab(State(state): State<state::State>, pod_name: String) {
    println!("{}", pod_name);
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");

    let dp = DeleteParams::default();
    pods.delete(&pod_name, &dp)
        .await
        .expect("deleting went wrong");
}

pub async fn status_lab(State(state): State<state::State>, pod_name: String) -> String{
    let pods: Api<Pod> = Api::namespaced(state.kube_client.clone(), "default");
    let pod = pods.get(pod_name.as_str()).await.expect("PULAMEA");
    pod.status.expect("PIZDAMATI").phase.expect("COAIE")
}