use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

mod routes;
mod models;

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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8085")
        .await
        .unwrap();

    println!("Lab API Service running on http://localhost:8085");

    axum::serve(listener, app)
        .await
        .unwrap();
}
