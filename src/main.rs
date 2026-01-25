use kube::{config::AuthInfo, Client, Config};
use rustls_pemfile::certs;
use std::io::BufReader;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod models;
mod routes;
mod services;

#[cfg(test)]
mod tests;

const DEFAULT_PORT: &str = "8085";
const GKE_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

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

    let kube_client = create_gke_client(&token_provider).await?;

    Ok(models::State {
        token_provider,
        kube_client,
    })
}

/// Creates a Kubernetes client for GKE.
///
/// When running locally with kubeconfig, uses the default config.
/// When running on Cloud Run (with GKE_* env vars), connects to GKE using GCP auth.
///
/// Required env vars for Cloud Run:
/// - GKE_CLUSTER_ENDPOINT: The GKE cluster API endpoint (e.g., https://34.xxx.xxx.xxx)
/// - GKE_CLUSTER_CA: Base64-encoded cluster CA certificate
async fn create_gke_client(
    token_provider: &std::sync::Arc<dyn gcp_auth::TokenProvider>,
) -> Result<Client, String> {
    let gke_endpoint = std::env::var("GKE_CLUSTER_ENDPOINT");
    let gke_ca = std::env::var("GKE_CLUSTER_CA");

    match (gke_endpoint, gke_ca) {
        (Ok(endpoint), Ok(ca_input)) => {
            info!("Using GKE cluster endpoint: {}", endpoint);

            // Accept base64 or PEM input and extract DER cert bytes
            let pem_bytes = if ca_input.contains("BEGIN CERTIFICATE") {
                ca_input.into_bytes()
            } else {
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &ca_input)
                    .map_err(|e| format!("Failed to decode GKE_CLUSTER_CA: {}", e))?
            };

            let mut reader = BufReader::new(pem_bytes.as_slice());
            let certs = certs(&mut reader)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to parse CA certificate PEM: {}", e))?;
            let Some(first) = certs.first() else {
                return Err("No certificates found in GKE_CLUSTER_CA".to_string());
            };
            let ca_der = first.as_ref().to_vec();

            let token = token_provider
                .token(&[GKE_SCOPE])
                .await
                .map_err(|e| format!("Failed to get GCP token for GKE: {}", e))?;

            // Normalize endpoint to ensure it has a scheme for kube client
            let endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                endpoint
            } else {
                format!("https://{}", endpoint)
            };

            let mut config = Config::new(
                endpoint
                    .parse()
                    .map_err(|e| format!("Invalid GKE_CLUSTER_ENDPOINT URL: {}", e))?,
            );
            config.root_cert = Some(vec![ca_der]);
            config.auth_info = AuthInfo {
                token: Some(secrecy::SecretString::new(
                    token.as_str().to_string().into(),
                )),
                ..Default::default()
            };

            Client::try_from(config).map_err(|e| format!("Failed to create GKE client: {}", e))
        }
        _ => {
            info!("Using default kubeconfig (local development mode)");
            Client::try_default()
                .await
                .map_err(|e| format!("Kubernetes client init failed: {}", e))
        }
    }
}
