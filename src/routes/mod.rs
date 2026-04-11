mod spawn;
mod web;
mod web_shell;

// Public for testing
pub mod health;

use axum::{
    routing::{any, get, post},
    Router,
};

use crate::models::State;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabApiRole {
    RuntimeApi,
    WebProxy,
}

impl LabApiRole {
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("LAB_API_ROLE").ok().as_deref())
    }

    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            Some("runtime-api") => Ok(Self::RuntimeApi),
            Some("web-proxy") => Ok(Self::WebProxy),
            Some(other) => Err(format!(
                "Unsupported LAB_API_ROLE '{other}'. Expected 'runtime-api' or 'web-proxy'."
            )),
            None => Err("LAB_API_ROLE must be set to 'runtime-api' or 'web-proxy'.".to_string()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeApi => "runtime-api",
            Self::WebProxy => "web-proxy",
        }
    }
}

pub fn init_routes(role: LabApiRole) -> Router<State> {
    Router::new()
        .route("/health", get(health::health))
        .merge(match role {
            LabApiRole::RuntimeApi => runtime_api_routes(),
            LabApiRole::WebProxy => web_proxy_routes(),
        })
}

fn runtime_api_routes() -> Router<State> {
    Router::new()
        .route("/spawn", post(spawn::spawn_lab))
        .route("/spawn/stop", post(spawn::stop_lab))
        .route("/spawn/status/{container_id}", get(spawn::status_lab))
        // open-session creates the browser-facing LAB-WEB cookie before the learner
        // is redirected to the actual /web/{container_id} runtime route.
        .route(
            "/web/open-session/{session_id}",
            post(web::open_web_session),
        )
        // /web/{container_id} then carries the normal HTTP traffic for the running
        // lab application after the bootstrap step has completed.
        .route("/web/{container_id}", any(web::runtime_web_request))
        .route("/web/{container_id}/", any(web::runtime_web_request))
        .route("/web/{container_id}/{*path}", any(web::runtime_web_request))
        .route(
            "/spawn/webshell/{pod_name}",
            get(web_shell::lab_terminal_ws),
        )
}

fn web_proxy_routes() -> Router<State> {
    Router::new()
        // The web-proxy role receives already-authenticated LAB-WEB traffic from
        // runtime-api and only forwards it to the per-session Kubernetes Service.
        .route("/web/{container_id}", any(web::web_proxy_root_request))
        .route("/web/{container_id}/", any(web::web_proxy_root_request))
        .route(
            "/web/{container_id}/{*path}",
            any(web::web_proxy_path_request),
        )
}

#[cfg(test)]
mod tests {
    use super::LabApiRole;

    #[test]
    fn lab_api_role_parse_accepts_supported_values() {
        assert_eq!(
            LabApiRole::parse(Some("runtime-api")).unwrap(),
            LabApiRole::RuntimeApi
        );
        assert_eq!(
            LabApiRole::parse(Some("web-proxy")).unwrap(),
            LabApiRole::WebProxy
        );
    }

    #[test]
    fn lab_api_role_parse_rejects_missing_or_invalid_values() {
        assert!(LabApiRole::parse(None).is_err());
        assert!(LabApiRole::parse(Some("")).is_err());
        assert!(LabApiRole::parse(Some("proxy")).is_err());
    }
}
