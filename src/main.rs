use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;
use gcp_auth;

mod models;
mod routes;

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

    let app = init_routes().layer(cors);

    let provider = gcp_auth::provider().await.unwrap();
    let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
    let token = provider.token(scopes).await.unwrap();


    let listener = tokio::net::TcpListener::bind("0.0.0.0:8085").await.unwrap();

    println!("Lab API Service running on http://localhost:8085");

    axum::serve(listener, app).await.unwrap();
}
