use std::sync::Arc;

use gcp_auth::TokenProvider;
use kube::Client;
use reqwest::Client as HttpClient;

#[derive(Clone)]
pub struct State {
    pub token_provider: Option<Arc<dyn TokenProvider>>,
    pub kube_client: Client,
    pub http_client: HttpClient,
    pub local_mode: bool,
}
