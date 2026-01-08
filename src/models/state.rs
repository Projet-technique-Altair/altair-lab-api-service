//use gcp_auth::TokenProvider;
//use std::sync::Arc;
use kube::Client;

#[derive(Clone)]
pub struct State {
    //pub token_provider: Arc<dyn TokenProvider>,
    pub kube_client: Client,
}
