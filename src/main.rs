use kube::Client;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod models;
mod routes;
mod services;

const DEFAULT_PORT: &str = "8085";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let state = match init_state().await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to initialize application state: {}", e);
            std::process::exit(1);
        }
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = routes::init_routes().layer(cors).with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    info!("Server started on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}

async fn init_state() -> Result<models::State, String> {
    let token_provider = gcp_auth::provider().await.map_err(|e| {
        format!(
            "GCP auth init failed: {}. Ensure GOOGLE_APPLICATION_CREDENTIALS is set.",
            e
        )
    })?;

    let kube_client = Client::try_default()
        .await
        .map_err(|e| format!("Kubernetes client init failed: {}", e))?;

    Ok(models::State {
        token_provider,
        kube_client,
    })
}
