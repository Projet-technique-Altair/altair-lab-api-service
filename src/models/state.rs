use gcp_auth::TokenProvider;
use kube::Client;
use std::sync::Arc;

#[derive(Clone)]
pub struct State {
    pub token_provider: Arc<dyn TokenProvider>,
    pub kube_client: Client,
}
