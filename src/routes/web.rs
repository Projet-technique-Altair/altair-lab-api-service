use std::time::Duration;

use axum::{
    body::Body,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderName, StatusCode, Uri},
    response::Response,
};

use crate::models::State as AppState;

pub async fn runtime_web_request(
    State(_state): State<AppState>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let proxy_base_url = std::env::var("LAB_WEB_PROXY_BASE_URL").map_err(|_| {
        // The public runtime-api stays dumb here: it only forwards to the
        // internal web-proxy base URL that infra already provisioned.
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let target_url = build_runtime_proxy_target_url(&proxy_base_url, &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_get_request(target_url, headers).await
}

pub async fn web_proxy_root_request(
    State(_state): State<AppState>,
    Path(container_id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let target_url = build_session_service_target_url(&container_id, None, &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_get_request(target_url, headers).await
}

pub async fn web_proxy_path_request(
    State(_state): State<AppState>,
    Path((container_id, path)): Path<(String, String)>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let target_url = build_session_service_target_url(&container_id, Some(&path), &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_get_request(target_url, headers).await
}

fn build_runtime_proxy_target_url(base_url: &str, original_uri: &Uri) -> Result<String, String> {
    let mut target_url = format!("{}{}", base_url.trim_end_matches('/'), original_uri.path());

    if let Some(query) = original_uri.query() {
        target_url.push('?');
        target_url.push_str(query);
    }

    Ok(target_url)
}

fn build_session_service_target_url(
    container_id: &str,
    path: Option<&str>,
    original_uri: &Uri,
) -> Result<String, String> {
    let namespace =
        std::env::var("WEB_PROXY_NAMESPACE").unwrap_or_else(|_| "labs-web".to_string());
    let service_suffix =
        std::env::var("WEB_PROXY_SERVICE_SUFFIX").unwrap_or_else(|_| "-web".to_string());

    // The web-proxy computes the stable Service DNS name directly from the
    // session container_id instead of querying Kubernetes on each request.
    let mut target_url = format!(
        "http://{}{service_suffix}.{namespace}.svc.cluster.local",
        container_id
    );

    let stripped_path = match path {
        Some(value) if !value.is_empty() => format!("/{}", value.trim_start_matches('/')),
        _ => "/".to_string(),
    };
    target_url.push_str(&stripped_path);

    if let Some(query) = original_uri.query() {
        target_url.push('?');
        target_url.push_str(query);
    }

    Ok(target_url)
}

async fn proxy_get_request(
    target_url: String,
    incoming_headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let timeout_secs = std::env::var("WEB_PROXY_REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut request = client.get(&target_url);

    // Forward only end-to-end headers. Hop-by-hop transport headers are rebuilt
    // by the HTTP client/server layers and should not be copied blindly.
    for (name, value) in &incoming_headers {
        if !is_hop_by_hop_header(name) {
            request = request.header(name, value);
        }
    }

    let upstream = request.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = upstream.status();
    let response_headers = upstream.headers().clone();
    let body = upstream
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut response = Response::builder().status(status);

    for (name, value) in &response_headers {
        if !is_hop_by_hop_header(name) {
            response = response.header(name, value);
        }
    }

    response
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

#[cfg(test)]
mod tests {
    use super::{build_runtime_proxy_target_url, build_session_service_target_url};
    use axum::http::Uri;

    #[test]
    fn runtime_proxy_target_keeps_path_and_query() {
        // The runtime-api should forward the original public /web path unchanged
        // to the internal web-proxy base URL.
        let uri: Uri = "/web/ctf-session-123/assets/app.js?lang=en".parse().unwrap();
        let target =
            build_runtime_proxy_target_url("http://10.200.0.14", &uri).unwrap();

        assert_eq!(
            target,
            "http://10.200.0.14/web/ctf-session-123/assets/app.js?lang=en"
        );
    }

    #[test]
    fn session_service_target_rewrites_root_path() {
        // The internal web-proxy strips the /web/{container_id} prefix and sends
        // root requests to the session Service root path.
        let uri: Uri = "/web/ctf-session-123".parse().unwrap();
        let target = build_session_service_target_url("ctf-session-123", None, &uri).unwrap();

        assert_eq!(
            target,
            "http://ctf-session-123-web.labs-web.svc.cluster.local/"
        );
    }

    #[test]
    fn session_service_target_rewrites_nested_path_and_query() {
        // Nested assets and query strings must survive the rewrite to the
        // per-session Service DNS endpoint.
        let uri: Uri = "/web/ctf-session-123/assets/app.js?lang=en".parse().unwrap();
        let target = build_session_service_target_url(
            "ctf-session-123",
            Some("assets/app.js"),
            &uri,
        )
        .unwrap();

        assert_eq!(
            target,
            "http://ctf-session-123-web.labs-web.svc.cluster.local/assets/app.js?lang=en"
        );
    }
}
