// use gcp_auth;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;
use kube::Client;

mod auth;
mod models;
mod routes;
mod services;

use crate::routes::init_routes;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let state = crate::models::state::State {
        //token_provider: gcp_auth::provider().await.unwrap(),
        kube_client: Client::try_default().await.expect("Something is rotten in the state of Alabama and idk what"),
    };

    let app = init_routes().layer(cors).with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8085").await.unwrap();

    println!("Lab API Service running on http://localhost:8085");

    axum::serve(listener, app).await.unwrap();
}
