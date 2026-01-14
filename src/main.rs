use kube::Client;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

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

    let token_provider = gcp_auth::provider().await.expect(
        "Failed to initialize GCP auth provider. Make sure GOOGLE_APPLICATION_CREDENTIALS is set or running on GCP.",
    );

    let state = crate::models::state::State {
        token_provider,
        kube_client: Client::try_default()
            .await
            .expect("An error has occured while trying to initialize the Kubernetes cluster connection, possibly the credentials are not there."),
    };

    let app = init_routes().layer(cors).with_state(state);

    let port = std::env::var("PORT").unwrap_or("8085".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();

    println!("The service started on port: {}", port);

    axum::serve(listener, app).await.unwrap();
}
