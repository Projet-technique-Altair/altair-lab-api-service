use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Response, StatusCode},
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Url;
use serde::{Deserialize, Serialize};
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
    let target_url = build_sessions_ms_runtime_lookup_url(&sessions_ms_base, session_id)?;

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

fn build_sessions_ms_runtime_lookup_url(
    sessions_ms_base: &str,
    session_id: Uuid,
) -> Result<Url, StatusCode> {
    let mut url = Url::parse(sessions_ms_base).map_err(|_| StatusCode::BAD_GATEWAY)?;
    validate_sensitive_internal_url(&url)?;
    url.set_path(&format!("/internal/sessions/{session_id}/web-runtime"));
    url.set_query(None);
    Ok(url)
}

fn validate_sensitive_internal_url(url: &Url) -> Result<(), StatusCode> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(url) => Ok(()),
        _ => Err(StatusCode::BAD_GATEWAY),
    }
}

fn is_loopback_host(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
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

fn build_open_web_redirect_url(app_base_url: &str, container_id: &str) -> String {
    format!(
        "{}/web/{}/",
        app_base_url.trim_end_matches('/'),
        container_id
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_open_web_redirect_url, build_sessions_ms_runtime_lookup_url, is_loopback_host,
    };
    use axum::http::StatusCode;
    use reqwest::Url;
    use uuid::Uuid;

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
    fn sessions_ms_lookup_url_keeps_trusted_host() {
        let session_id = Uuid::parse_str("9bc97880-f720-41c1-9e8a-a2010e2f02c2").unwrap();
        let url =
            build_sessions_ms_runtime_lookup_url("https://sessions.example.test/base", session_id)
                .unwrap();

        assert_eq!(
            url,
            Url::parse(
                "https://sessions.example.test/internal/sessions/9bc97880-f720-41c1-9e8a-a2010e2f02c2/web-runtime"
            )
            .unwrap()
        );
    }

    #[test]
    fn sessions_ms_lookup_url_accepts_local_http_for_development() {
        let session_id = Uuid::parse_str("9bc97880-f720-41c1-9e8a-a2010e2f02c2").unwrap();
        let url =
            build_sessions_ms_runtime_lookup_url("http://localhost:3003", session_id).unwrap();

        assert_eq!(
            url,
            Url::parse(
                "http://localhost:3003/internal/sessions/9bc97880-f720-41c1-9e8a-a2010e2f02c2/web-runtime"
            )
            .unwrap()
        );
    }

    #[test]
    fn sessions_ms_lookup_url_rejects_remote_http() {
        let session_id = Uuid::parse_str("9bc97880-f720-41c1-9e8a-a2010e2f02c2").unwrap();

        assert_eq!(
            build_sessions_ms_runtime_lookup_url("http://sessions.example.test", session_id)
                .unwrap_err(),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn loopback_detection_accepts_local_targets_only() {
        assert!(is_loopback_host(
            &Url::parse("http://localhost:3003").unwrap()
        ));
        assert!(is_loopback_host(
            &Url::parse("http://127.0.0.1:3003").unwrap()
        ));
        assert!(!is_loopback_host(
            &Url::parse("https://sessions.example.test").unwrap()
        ));
    }
}
