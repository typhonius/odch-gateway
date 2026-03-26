use std::net::ToSocketAddrs;

use axum::extract::{Path, State};
use axum::Json;
use url::Url;

use crate::error::AppError;
use crate::state::AppState;
use crate::webhook::manager::WebhookInput;

/// Validate a webhook URL to prevent SSRF attacks.
/// Rejects non-HTTP(S) schemes, localhost, and private/reserved IP ranges.
fn validate_webhook_url(raw_url: &str) -> Result<(), AppError> {
    let parsed =
        Url::parse(raw_url).map_err(|_| AppError::BadRequest("Invalid URL format".to_string()))?;

    // Must be http or https
    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(AppError::BadRequest(
                "Webhook URL must use http or https scheme".to_string(),
            ));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::BadRequest("URL must have a host".to_string()))?;

    // Reject localhost
    if host == "localhost" {
        return Err(AppError::BadRequest(
            "Webhook URL must not target localhost".to_string(),
        ));
    }

    // Resolve hostname and check all resolved IPs
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);
    if let Ok(addrs) = addr_str.to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip();
            if ip.is_loopback() || is_private_ip(&ip) {
                return Err(AppError::BadRequest(
                    "Webhook URL must not target private or reserved IP addresses".to_string(),
                ));
            }
        }
    }

    Ok(())
}

/// Check if an IP address is in a private or reserved range.
fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            // 10.0.0.0/8
            octets[0] == 10
            // 172.16.0.0/12
            || (octets[0] == 172 && (16..=31).contains(&octets[1]))
            // 192.168.0.0/16
            || (octets[0] == 192 && octets[1] == 168)
            // 127.0.0.0/8 (loopback)
            || octets[0] == 127
            // 169.254.0.0/16 (link-local)
            || (octets[0] == 169 && octets[1] == 254)
            // 0.0.0.0/8
            || octets[0] == 0
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_unspecified()
                // IPv4-mapped IPv6 addresses - check the embedded IPv4
                || {
                    if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                        is_private_ip(&std::net::IpAddr::V4(ipv4))
                    } else {
                        false
                    }
                }
        }
    }
}

/// GET /api/webhooks
///
/// List all registered webhooks.
pub async fn list_webhooks(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let webhooks = state.webhook_manager.list().await;
    Ok(Json(serde_json::json!({
        "webhooks": webhooks,
        "count": webhooks.len(),
    })))
}

/// POST /api/webhooks
///
/// Create a new webhook.
pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<WebhookInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.url.is_empty() {
        return Err(AppError::BadRequest("URL is required".to_string()));
    }
    validate_webhook_url(&body.url)?;

    let webhook = state.webhook_manager.create(body).await?;

    Ok(Json(serde_json::json!({
        "webhook": webhook,
    })))
}

/// PUT /api/webhooks/:id
///
/// Update an existing webhook.
pub async fn update_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WebhookInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.url.is_empty() {
        return Err(AppError::BadRequest("URL is required".to_string()));
    }
    validate_webhook_url(&body.url)?;

    let webhook = state.webhook_manager.update(&id, body).await?;

    Ok(Json(serde_json::json!({
        "webhook": webhook,
    })))
}

/// DELETE /api/webhooks/:id
///
/// Delete a webhook.
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.webhook_manager.delete(&id).await?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "id": id,
    })))
}
