use std::sync::Arc;

use gcp_auth::TokenProvider;
use kube::Client;

#[derive(Clone)]
pub struct State {
    pub token_provider: Option<Arc<dyn TokenProvider>>,
    pub kube_client: Client,
    pub local_mode: bool,
}
