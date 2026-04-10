use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderName, HeaderValue, Request, StatusCode, Uri,
    },
    response::Response,
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header as JwtHeader};
use serde::{Deserialize, Serialize};

use crate::models::State as AppState;

#[derive(Debug, Deserialize)]
struct SessionsRuntimeLookupEnvelope {
    data: SessionsRuntimeLookup,
}

#[derive(Debug, Deserialize)]
struct SessionsRuntimeLookup {
    session_id: String,
    user_id: String,
    container_id: String,
    runtime_kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WebSessionClaims {
    kind: String,
    cid: String,
    sid: String,
    uid: String,
    iat: usize,
    exp: usize,
}

pub async fn bootstrap_web_session(
    State(_state): State<AppState>,
    Path(container_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let caller_user_id = headers
        .get("x-altair-user-id")
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let sessions_ms_url =
        std::env::var("SESSIONS_MS_URL").unwrap_or_else(|_| "http://localhost:3003".to_string());
    let lookup_url = format!(
        "{}/internal/runtime/by-container/{}",
        sessions_ms_url.trim_end_matches('/'),
        container_id
    );

    // The bootstrap route asks sessions-ms for the live runtime ownership
    // snapshot before issuing a browser cookie for the iframe path.
    let lookup_response = reqwest::Client::new()
        .get(&lookup_url)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = lookup_response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(StatusCode::NOT_FOUND);
    }
    if !status.is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let lookup = lookup_response
        .json::<SessionsRuntimeLookupEnvelope>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if lookup.data.user_id != caller_user_id {
        return Err(StatusCode::FORBIDDEN);
    }
    if lookup.data.runtime_kind != "web" || lookup.data.container_id != container_id {
        return Err(StatusCode::CONFLICT);
    }

    let cookie_name = web_cookie_name();
    let cookie_ttl_seconds = web_cookie_ttl_seconds();
    let cookie_value = build_signed_web_cookie(
        &lookup.data.container_id,
        &lookup.data.session_id,
        &lookup.data.user_id,
        cookie_ttl_seconds,
    )?;
    let set_cookie_header =
        build_set_cookie_header(&cookie_name, &cookie_value, cookie_ttl_seconds);

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(SET_COOKIE, set_cookie_header)
        .body(Body::empty())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn runtime_web_request(
    State(_state): State<AppState>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let proxy_base_url = std::env::var("LAB_WEB_PROXY_BASE_URL").map_err(|_| {
        // The public runtime-api stays dumb here: it only forwards to the
        // internal web-proxy base URL that infra already provisioned.
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let target_url = build_runtime_proxy_target_url(&proxy_base_url, request.uri())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
}

pub async fn web_proxy_root_request(
    State(_state): State<AppState>,
    Path(container_id): Path<String>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let target_url = build_session_service_target_url(&container_id, None, request.uri())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
}

pub async fn web_proxy_path_request(
    State(_state): State<AppState>,
    Path((container_id, path)): Path<(String, String)>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let target_url = build_session_service_target_url(&container_id, Some(&path), request.uri())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
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
    let namespace = std::env::var("WEB_PROXY_NAMESPACE").unwrap_or_else(|_| "labs-web".to_string());
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

async fn proxy_request(
    target_url: String,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let timeout_secs = std::env::var("WEB_PROXY_REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let method = reqwest::Method::from_bytes(request.method().as_str().as_bytes())
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let headers = request.headers().clone();
    let body = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut upstream = client.request(method, &target_url);

    // The platform auth boundary ends before the request reaches the runtime
    // pod. Only end-to-end application headers should continue downstream.
    for (name, value) in &headers {
        if is_hop_by_hop_header(name) || is_platform_header(name) {
            continue;
        }

        if *name == COOKIE {
            if let Some(filtered_cookie) = filter_cookie_header(value) {
                upstream = upstream.header(name, filtered_cookie);
            }
            continue;
        }

        upstream = upstream.header(name, value);
    }

    let upstream_response = upstream
        .body(body)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = upstream_response.status();
    let response_headers = upstream_response.headers().clone();
    let response_body = upstream_response
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
        .body(Body::from(response_body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_signed_web_cookie(
    container_id: &str,
    session_id: &str,
    user_id: &str,
    ttl_seconds: u64,
) -> Result<String, StatusCode> {
    let issued_at = now_unix_timestamp()?;
    let expires_at = issued_at
        .checked_add(ttl_seconds as usize)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let secret = web_cookie_signing_secret()?;

    let claims = WebSessionClaims {
        kind: "lab_web".to_string(),
        cid: container_id.to_string(),
        sid: session_id.to_string(),
        uid: user_id.to_string(),
        iat: issued_at,
        exp: expires_at,
    };

    encode(
        &JwtHeader::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_set_cookie_header(name: &str, value: &str, ttl_seconds: u64) -> String {
    format!(
        "{name}={value}; Max-Age={ttl_seconds}; Path=/lab-api/web; HttpOnly; Secure; SameSite=Lax"
    )
}

fn filter_cookie_header(value: &HeaderValue) -> Option<String> {
    let raw = value.to_str().ok()?;
    let auth_cookie_name = web_cookie_name();
    let filtered: Vec<&str> = raw
        .split(';')
        .map(str::trim)
        .filter(|cookie| !cookie.is_empty())
        .filter(|cookie| {
            !cookie
                .split('=')
                .next()
                .map(|name| name.trim() == auth_cookie_name)
                .unwrap_or(false)
        })
        .collect();

    if filtered.is_empty() {
        None
    } else {
        Some(filtered.join("; "))
    }
}

fn is_platform_header(name: &HeaderName) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    lower == "authorization" || lower.starts_with("x-altair-")
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

fn web_cookie_name() -> String {
    std::env::var("LAB_WEB_COOKIE_NAME").unwrap_or_else(|_| "altair_web_session".to_string())
}

fn web_cookie_ttl_seconds() -> u64 {
    std::env::var("LAB_WEB_COOKIE_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3600)
}

fn web_cookie_signing_secret() -> Result<String, StatusCode> {
    std::env::var("LAB_WEB_COOKIE_SIGNING_SECRET").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn now_unix_timestamp() -> Result<usize, StatusCode> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as usize)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod tests {
    use super::{
        build_runtime_proxy_target_url, build_session_service_target_url, build_set_cookie_header,
        filter_cookie_header,
    };
    use axum::http::{HeaderValue, Uri};

    #[test]
    fn runtime_proxy_target_keeps_path_and_query() {
        // The runtime-api should forward the original public /web path unchanged
        // to the internal web-proxy base URL.
        let uri: Uri = "/web/ctf-session-123/assets/app.js?lang=en"
            .parse()
            .unwrap();
        let target = build_runtime_proxy_target_url("http://10.200.0.14", &uri).unwrap();

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
        let uri: Uri = "/web/ctf-session-123/assets/app.js?lang=en"
            .parse()
            .unwrap();
        let target =
            build_session_service_target_url("ctf-session-123", Some("assets/app.js"), &uri)
                .unwrap();

        assert_eq!(
            target,
            "http://ctf-session-123-web.labs-web.svc.cluster.local/assets/app.js?lang=en"
        );
    }

    #[test]
    fn filter_cookie_header_removes_platform_cookie_only() {
        std::env::set_var("LAB_WEB_COOKIE_NAME", "altair_web_session");
        let header_value =
            HeaderValue::from_static("altair_web_session=abc; app_session=xyz; theme=dark");

        let filtered = filter_cookie_header(&header_value).unwrap();

        assert_eq!(filtered, "app_session=xyz; theme=dark");
    }

    #[test]
    fn set_cookie_header_uses_web_path_scope() {
        let header = build_set_cookie_header("altair_web_session", "signed-token", 3600);

        assert!(header.contains("Path=/lab-api/web"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Secure"));
    }
}
