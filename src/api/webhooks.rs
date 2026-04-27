use std::net::ToSocketAddrs;

use axum::extract::{Path, State};
use axum::Json;
use url::Url;

use crate::error::AppError;
use crate::state::AppState;
use crate::webhook::manager::WebhookInput;

const VALID_EVENT_TYPES: &[&str] = &[
    "Chat",
    "UserJoin",
    "UserQuit",
    "UserInfo",
    "HubName",
    "OpListUpdate",
    "Kick",
    "GatewayStatus",
];

fn validate_event_types(events: &[String]) -> Result<(), AppError> {
    for event in events {
        if !VALID_EVENT_TYPES.contains(&event.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unknown event type '{}'. Valid types: {}",
                event,
                VALID_EVENT_TYPES.join(", ")
            )));
        }
    }
    Ok(())
}

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
    // DNS resolution MUST succeed — if it fails, reject (prevents DNS rebinding
    // where an initially-unresolvable host later resolves to a private IP)
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|_| {
            AppError::BadRequest("Webhook URL hostname could not be resolved".to_string())
        })?
        .collect();

    if addrs.is_empty() {
        return Err(AppError::BadRequest(
            "Webhook URL hostname resolved to no addresses".to_string(),
        ));
    }

    for addr in &addrs {
        let ip = addr.ip();
        if ip.is_loopback() || is_private_ip(&ip) {
            return Err(AppError::BadRequest(
                "Webhook URL must not target private or reserved IP addresses".to_string(),
            ));
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
            let segs = ipv6.segments();
            ipv6.is_loopback()
                || ipv6.is_unspecified()
                // fe80::/10 — link-local
                || (segs[0] & 0xffc0) == 0xfe80
                // fc00::/7 — unique local addresses (ULA, IPv6 equivalent of RFC1918)
                || (segs[0] & 0xfe00) == 0xfc00
                // IPv4-mapped IPv6 addresses (::ffff:x.x.x.x) — check the embedded IPv4
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
    if !body.events.is_empty() {
        validate_event_types(&body.events)?;
    }

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
    if !body.events.is_empty() {
        validate_event_types(&body.events)?;
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_event_type_rejected() {
        let events = vec!["Chat".to_string(), "InvalidType".to_string()];
        let result = validate_event_types(&events);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("InvalidType"));
    }

    #[test]
    fn test_valid_event_types_accepted() {
        let events = vec![
            "Chat".to_string(),
            "UserJoin".to_string(),
            "Kick".to_string(),
        ];
        let result = validate_event_types(&events);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_events_allowed() {
        let events: Vec<String> = vec![];
        // Empty events should not be passed to validate_event_types,
        // but even if they are, it should succeed (no invalid events).
        let result = validate_event_types(&events);
        assert!(result.is_ok());
    }
}
