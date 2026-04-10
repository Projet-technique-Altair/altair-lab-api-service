use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::{HeaderMap, HeaderName, Request, Response, StatusCode, Uri},
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;

use crate::models::State as AppState;

const HDR_USER_ID: &str = "x-altair-user-id";
const DEFAULT_COOKIE_NAME: &str = "altair_web_session";
const DEFAULT_COOKIE_TTL_SECONDS: u64 = 3600;

#[derive(Deserialize)]
struct SessionsApiResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct WebRuntimeLookup {
    user_id: Uuid,
    runtime_kind: String,
    container_id: String,
    status: String,
}

#[derive(Serialize)]
struct OpenWebSessionResponse {
    redirect_url: String,
}

#[derive(Serialize)]
struct OpenWebSessionApiResponse {
    success: bool,
    data: OpenWebSessionResponse,
}

#[derive(Serialize, Deserialize)]
struct LabWebCookieClaims {
    kind: String,
    cid: String,
    uid: String,
    exp: usize,
}

pub async fn open_web_session(
    State(_state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // open-session exists only to prepare the browser-facing LAB-WEB cookie
    // before the learner is redirected to the real /web/{container_id} route.
    let user_id = extract_user_id(&headers)?;
    let runtime = fetch_web_runtime(session_id).await?;

    if runtime.user_id != user_id {
        return Err(StatusCode::FORBIDDEN);
    }

    if runtime.runtime_kind != "web" {
        return Err(StatusCode::BAD_REQUEST);
    }

    if runtime.status != "running" {
        return Err(StatusCode::CONFLICT);
    }

    let cookie_name =
        std::env::var("LAB_WEB_COOKIE_NAME").unwrap_or_else(|_| DEFAULT_COOKIE_NAME.to_string());
    let ttl_seconds = std::env::var("LAB_WEB_COOKIE_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_COOKIE_TTL_SECONDS);
    let signing_secret = std::env::var("LAB_WEB_COOKIE_SIGNING_SECRET")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let claims = LabWebCookieClaims {
        kind: "lab_web".to_string(),
        cid: runtime.container_id.clone(),
        uid: runtime.user_id.to_string(),
        exp: current_unix_timestamp(ttl_seconds)?,
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(signing_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let cookie_value = build_lab_web_cookie(&cookie_name, &token, ttl_seconds);
    let app_base_url =
        std::env::var("LAB_APP_BASE_URL").unwrap_or_else(|_| "http://localhost:8085".to_string());

    let payload = serde_json::to_vec(&OpenWebSessionApiResponse {
        success: true,
        data: OpenWebSessionResponse {
            redirect_url: build_open_web_redirect_url(&app_base_url, &runtime.container_id),
        },
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("set-cookie", cookie_value)
        .body(Body::from(payload))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn runtime_web_request(
    State(_state): State<AppState>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    // /web/{container_id} is the public runtime path used after open-session has
    // already created the browser cookie; from here lab-api only proxies traffic.
    let proxy_base_url =
        std::env::var("LAB_WEB_PROXY_BASE_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let original_uri = request.uri().clone();
    let target_url = build_runtime_proxy_target_url(&proxy_base_url, &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
}

pub async fn web_proxy_root_request(
    State(_state): State<AppState>,
    Path(container_id): Path<String>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    // The web-proxy role resolves the per-session Kubernetes Service and forwards
    // the already-authenticated LAB-WEB request to the actual runtime container.
    let original_uri = request.uri().clone();
    let target_url = build_session_service_target_url(&container_id, None, &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
}

pub async fn web_proxy_path_request(
    State(_state): State<AppState>,
    Path((container_id, path)): Path<(String, String)>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let original_uri = request.uri().clone();
    let target_url = build_session_service_target_url(&container_id, Some(&path), &original_uri)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    proxy_request(target_url, request).await
}

fn extract_user_id(headers: &HeaderMap) -> Result<Uuid, StatusCode> {
    headers
        .get(HDR_USER_ID)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(StatusCode::UNAUTHORIZED)
}

async fn fetch_web_runtime(session_id: Uuid) -> Result<WebRuntimeLookup, StatusCode> {
    let sessions_ms_base =
        std::env::var("SESSIONS_MS_URL").unwrap_or_else(|_| "http://localhost:3003".to_string());
    let target_url = format!(
        "{}/internal/sessions/{}/web-runtime",
        sessions_ms_base.trim_end_matches('/'),
        session_id
    );

    let response = reqwest::Client::new()
        .get(target_url)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
        );
    }

    let body = response
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    serde_json::from_slice::<SessionsApiResponse<WebRuntimeLookup>>(&body)
        .map(|payload| payload.data)
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

fn build_lab_web_cookie(name: &str, token: &str, ttl_seconds: u64) -> String {
    format!(
        "{name}={token}; HttpOnly; Secure; SameSite=Lax; Path=/lab-api/web; Max-Age={ttl_seconds}"
    )
}

fn current_unix_timestamp(ttl_seconds: u64) -> Result<usize, StatusCode> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(now.as_secs().saturating_add(ttl_seconds) as usize)
}

fn build_runtime_proxy_target_url(base_url: &str, original_uri: &Uri) -> Result<String, String> {
    let mut target_url = format!("{}{}", base_url.trim_end_matches('/'), original_uri.path());

    if let Some(query) = original_uri.query() {
        target_url.push('?');
        target_url.push_str(query);
    }

    Ok(target_url)
}

fn build_open_web_redirect_url(app_base_url: &str, container_id: &str) -> String {
    format!(
        "{}/web/{}/",
        app_base_url.trim_end_matches('/'),
        container_id
    )
}

fn build_session_service_target_url(
    container_id: &str,
    path: Option<&str>,
    original_uri: &Uri,
) -> Result<String, String> {
    let namespace = std::env::var("WEB_PROXY_NAMESPACE").unwrap_or_else(|_| "labs-web".to_string());
    let service_suffix =
        std::env::var("WEB_PROXY_SERVICE_SUFFIX").unwrap_or_else(|_| "-web".to_string());

    let mut target_url = if let Some(service_host) =
        read_session_service_env(container_id, &service_suffix, "SERVICE_HOST")
    {
        let service_port = read_session_service_env(container_id, &service_suffix, "SERVICE_PORT")
            .unwrap_or_else(|| "80".to_string());
        format!("http://{}:{}", service_host, service_port)
    } else {
        format!(
            "http://{}{service_suffix}.{namespace}.svc.cluster.local",
            container_id
        )
    };

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

fn read_session_service_env(
    container_id: &str,
    service_suffix: &str,
    suffix: &str,
) -> Option<String> {
    let service_name = format!("{}{}", container_id, service_suffix);
    let env_key = service_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    std::env::var(format!("{}_{}", env_key, suffix)).ok()
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
        .map_err(|error| {
            error!("failed to build LAB-WEB proxy client: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let method = reqwest::Method::from_bytes(request.method().as_str().as_bytes())
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let forwarded_headers: Vec<(HeaderName, axum::http::HeaderValue)> = request
        .headers()
        .iter()
        .filter(|(name, _)| should_forward_request_header(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    let body_bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|error| {
            error!(
                "failed to read LAB-WEB request body for {}: {}",
                target_url, error
            );
            StatusCode::BAD_GATEWAY
        })?;

    let mut outbound = client.request(method, &target_url).body(body_bytes);
    for (name, value) in forwarded_headers {
        outbound = outbound.header(name, value);
    }

    let upstream = outbound.send().await.map_err(|error| {
        error!(
            "LAB-WEB upstream request failed for {}: {}",
            target_url, error
        );
        StatusCode::BAD_GATEWAY
    })?;
    let status = upstream.status();
    let response_headers = upstream.headers().clone();
    let body = upstream.bytes().await.map_err(|error| {
        error!(
            "failed to read LAB-WEB upstream response body for {}: {}",
            target_url, error
        );
        StatusCode::BAD_GATEWAY
    })?;

    if !status.is_success() {
        let body_preview = String::from_utf8_lossy(&body);
        error!(
            "LAB-WEB upstream returned {} for {} with body preview: {}",
            status,
            target_url,
            body_preview.chars().take(200).collect::<String>()
        );
    }
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;

    for (name, value) in &response_headers {
        if !is_hop_by_hop_header(name) {
            response.headers_mut().append(name, value.clone());
        }
    }

    Ok(response)
}

fn should_forward_request_header(name: &HeaderName) -> bool {
    !is_hop_by_hop_header(name) && !is_platform_private_header(name)
}

fn is_platform_private_header(name: &HeaderName) -> bool {
    let header = name.as_str().to_ascii_lowercase();

    header == "authorization"
        || header == "cookie"
        || header == "origin"
        || header == "referer"
        || header.starts_with("x-altair-")
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
    use super::{
        build_open_web_redirect_url, build_runtime_proxy_target_url,
        build_session_service_target_url,
    };
    use axum::http::Uri;

    #[test]
    fn open_web_redirect_url_keeps_trailing_slash() {
        let target =
            build_open_web_redirect_url("https://api.altair-platform.space", "ctf-session-123");

        assert_eq!(
            target,
            "https://api.altair-platform.space/web/ctf-session-123/"
        );
    }

    #[test]
    fn runtime_proxy_target_keeps_path_and_query() {
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
        let uri: Uri = "/web/ctf-session-123".parse().unwrap();
        let target = build_session_service_target_url("ctf-session-123", None, &uri).unwrap();

        assert_eq!(
            target,
            "http://ctf-session-123-web.labs-web.svc.cluster.local/"
        );
    }

    #[test]
    fn session_service_target_rewrites_nested_path_and_query() {
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
}
