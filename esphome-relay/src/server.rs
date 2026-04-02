use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{info, warn};

use crate::cache::FirmwareCache;
use crate::status::StatusTracker;
use crate::webhook;

/// Shared application state passed to all handlers
#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<FirmwareCache>,
    pub status: StatusTracker,
    pub webhook_secret: Option<String>,
    pub refresh_notify: Arc<Notify>,
}

/// Build the Axum router with all routes
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ingress_panel))
        .route("/health", get(health))
        .route("/status", get(status_handler))
        .route("/devices", get(list_devices))
        .route("/devices/{name}/manifest.json", get(get_manifest))
        .route("/devices/{name}/firmware.ota.bin", get(get_firmware))
        .route("/refresh", post(refresh_handler))
        .route("/webhook/github", post(webhook_handler))
        .with_state(state)
}

// --- Handlers ---

async fn health() -> &'static str {
    "OK"
}

#[derive(Serialize)]
struct DeviceInfo {
    name: String,
    version: Option<String>,
}

async fn list_devices(State(state): State<AppState>) -> Json<Vec<DeviceInfo>> {
    let devices = state.cache.list_devices();
    let list: Vec<DeviceInfo> = devices
        .into_iter()
        .map(|name| {
            let version = state.cache.cached_version(&name);
            DeviceInfo { name, version }
        })
        .collect();
    Json(list)
}

async fn status_handler(State(state): State<AppState>) -> Json<Vec<crate::status::DeviceStatus>> {
    Json(state.status.all_devices())
}

async fn get_manifest(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .cache
        .read_manifest(&name)
        .map(|m| ([(axum::http::header::CONTENT_TYPE, "application/json")], m))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn get_firmware(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .cache
        .read_firmware(&name)
        .map(|data| {
            (
                [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                data,
            )
        })
        .ok_or(StatusCode::NOT_FOUND)
}

async fn refresh_handler(State(state): State<AppState>) -> &'static str {
    info!("Manual refresh triggered");
    state.refresh_notify.notify_one();
    "Refresh gestartet"
}

async fn webhook_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<&'static str, StatusCode> {
    let secret = state.webhook_secret.as_deref().ok_or_else(|| {
        warn!("Webhook received but no webhook_secret configured");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            warn!("Webhook missing X-Hub-Signature-256 header");
            StatusCode::UNAUTHORIZED
        })?;

    if !webhook::verify_signature(secret, &body, signature) {
        warn!("Webhook signature verification failed");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let event_type = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !webhook::is_release_published(event_type, &body) {
        info!("Webhook event ignored: type={}", event_type);
        return Ok("Event ignoriert");
    }

    info!("Webhook: release published event, triggering refresh");
    state.refresh_notify.notify_one();
    Ok("Refresh gestartet")
}

async fn ingress_panel(State(state): State<AppState>) -> Html<String> {
    let devices = state.status.all_devices();
    let mut rows = String::new();
    for d in &devices {
        let status_class = match d.sync_status {
            crate::status::SyncStatus::Aktuell => "ok",
            crate::status::SyncStatus::Veraltet => "warn",
            crate::status::SyncStatus::LadeFirmware => "loading",
            crate::status::SyncStatus::Fehler => "error",
            crate::status::SyncStatus::Unbekannt => "unknown",
        };
        rows.push_str(&format!(
            r#"<tr><td>{}</td><td>{}</td><td>{}</td><td class="{}">{}</td></tr>"#,
            d.name,
            d.local_version.as_deref().unwrap_or("–"),
            d.remote_version.as_deref().unwrap_or("–"),
            status_class,
            d.sync_status,
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ESPHome Firmware Relay</title>
<style>
  body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #fafafa; color: #333; }}
  h1 {{ font-size: 1.4rem; }}
  table {{ border-collapse: collapse; width: 100%; max-width: 800px; }}
  th, td {{ padding: 0.5rem 1rem; text-align: left; border-bottom: 1px solid #ddd; }}
  th {{ background: #f0f0f0; }}
  .ok {{ color: #2e7d32; font-weight: bold; }}
  .warn {{ color: #e65100; font-weight: bold; }}
  .loading {{ color: #1565c0; font-weight: bold; }}
  .error {{ color: #c62828; font-weight: bold; }}
  .unknown {{ color: #757575; }}
  .actions {{ margin-top: 1rem; }}
  button {{ padding: 0.5rem 1rem; cursor: pointer; border: 1px solid #ccc; border-radius: 4px; background: #fff; }}
  button:hover {{ background: #f0f0f0; }}
</style>
</head>
<body>
<h1>ESPHome Firmware Relay</h1>
<table>
  <thead><tr><th>Gerät</th><th>Lokal</th><th>Remote</th><th>Status</th></tr></thead>
  <tbody>{rows}</tbody>
</table>
{empty_msg}
<div class="actions">
  <button onclick="fetch('/refresh', {{method: 'POST'}}).then(() => setTimeout(() => location.reload(), 2000))">
    Refresh
  </button>
</div>
</body>
</html>"#,
        rows = rows,
        empty_msg = if devices.is_empty() {
            "<p>Keine Geräte im Cache.</p>"
        } else {
            ""
        },
    );
    Html(html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Arc::new(FirmwareCache::new(tmp.path().to_str().unwrap()));
        // Leak TempDir so it lives long enough for tests
        std::mem::forget(tmp);
        AppState {
            cache,
            status: StatusTracker::new(),
            webhook_secret: Some("test-secret".to_string()),
            refresh_notify: Arc::new(Notify::new()),
        }
    }

    async fn request(app: Router, method: &str, uri: &str) -> (StatusCode, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).to_string())
    }

    // --- Health ---

    #[tokio::test]
    async fn test_health() {
        let app = build_router(test_state());
        let (status, body) = request(app, "GET", "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "OK");
    }

    // --- Devices list ---

    #[tokio::test]
    async fn test_list_devices_empty() {
        let app = build_router(test_state());
        let (status, body) = request(app, "GET", "/devices").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "[]");
    }

    #[tokio::test]
    async fn test_list_devices_with_cached() {
        let state = test_state();
        state.cache.store_firmware("test-esp", &[0xAB]).unwrap();
        let app = build_router(state);
        let (status, body) = request(app, "GET", "/devices").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("test-esp"));
    }

    // --- Status ---

    #[tokio::test]
    async fn test_status_empty() {
        let app = build_router(test_state());
        let (status, body) = request(app, "GET", "/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "[]");
    }

    #[tokio::test]
    async fn test_status_with_devices() {
        let state = test_state();
        state.status.set_local_version("my-esp", "v1.0.0");
        state.status.set_remote_version("my-esp", "v1.0.0");
        let app = build_router(state);
        let (status, body) = request(app, "GET", "/status").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("my-esp"));
        assert!(body.contains("aktuell"));
    }

    // --- Manifest ---

    #[tokio::test]
    async fn test_get_manifest_not_found() {
        let app = build_router(test_state());
        let (status, _) = request(app, "GET", "/devices/nope/manifest.json").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_manifest_found() {
        let state = test_state();
        state
            .cache
            .store_manifest(
                "my-esp",
                r#"{"version":"1.0","builds":[{"path":"old"}]}"#,
                "http://localhost:8099",
            )
            .unwrap();
        let app = build_router(state);
        let (status, body) = request(app, "GET", "/devices/my-esp/manifest.json").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("1.0"));
    }

    // --- Firmware ---

    #[tokio::test]
    async fn test_get_firmware_not_found() {
        let app = build_router(test_state());
        let (status, _) = request(app, "GET", "/devices/nope/firmware.ota.bin").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_firmware_found() {
        let state = test_state();
        state.cache.store_firmware("my-esp", &[0xDE, 0xAD]).unwrap();
        let app = build_router(state);
        let req2 = Request::builder()
            .uri("/devices/my-esp/firmware.ota.bin")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req2).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], &[0xDE, 0xAD]);
    }

    // --- Refresh ---

    #[tokio::test]
    async fn test_refresh() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/refresh")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // --- Webhook ---

    #[tokio::test]
    async fn test_webhook_missing_signature() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/webhook/github")
            .header("x-github-event", "release")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_webhook_invalid_signature() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/webhook/github")
            .header("x-hub-signature-256", "sha256=bad")
            .header("x-github-event", "release")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_webhook_valid_but_wrong_event() {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let payload = br#"{"action":"completed"}"#;
        let mut mac = HmacSha256::new_from_slice(b"test-secret").unwrap();
        mac.update(payload);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/webhook/github")
            .header("x-hub-signature-256", sig)
            .header("x-github-event", "push")
            .body(Body::from(payload.to_vec()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("ignoriert"));
    }

    #[tokio::test]
    async fn test_webhook_valid_release_published() {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let payload = br#"{"action":"published","release":{"tag_name":"v1.0"}}"#;
        let mut mac = HmacSha256::new_from_slice(b"test-secret").unwrap();
        mac.update(payload);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/webhook/github")
            .header("x-hub-signature-256", sig)
            .header("x-github-event", "release")
            .body(Body::from(payload.to_vec()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("Refresh"));
    }

    // --- Ingress Panel ---

    #[tokio::test]
    async fn test_ingress_panel_empty() {
        let app = build_router(test_state());
        let (status, body) = request(app, "GET", "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("ESPHome Firmware Relay"));
        assert!(body.contains("Keine Geräte"));
    }

    #[tokio::test]
    async fn test_ingress_panel_with_devices() {
        let state = test_state();
        state.status.set_local_version("living-room", "v1.0.0");
        state.status.set_remote_version("living-room", "v2.0.0");
        let app = build_router(state);
        let (status, body) = request(app, "GET", "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("living-room"));
        assert!(body.contains("veraltet"));
    }
}
