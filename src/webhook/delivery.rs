use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{error, info, warn};

use crate::config::WebhookConfig;
use crate::event::HubEvent;
use crate::webhook::manager::{Webhook, WebhookManager};

type HmacSha256 = Hmac<Sha256>;

/// Compute HMAC-SHA256 signature for a payload.
fn sign_payload(secret: &str, payload: &[u8]) -> String {
    if secret.is_empty() {
        return String::new();
    }
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Deliver a single webhook with retries.
async fn deliver_one(
    client: &reqwest::Client,
    webhook: &Webhook,
    payload: &str,
    max_retries: u32,
    retry_delay: Duration,
) {
    let signature = sign_payload(&webhook.secret, payload.as_bytes());

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let delay = retry_delay * attempt;
            warn!(
                "Webhook {} delivery attempt {} (retrying in {:?})",
                webhook.id,
                attempt + 1,
                delay,
            );
            tokio::time::sleep(delay).await;
        }

        let mut req = client
            .post(&webhook.url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "odch-gateway/0.1.0");

        if !signature.is_empty() {
            req = req.header("X-Webhook-Signature", format!("sha256={}", signature));
        }

        match req.body(payload.to_string()).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    info!(
                        "Webhook {} delivered to {} (status {})",
                        webhook.id,
                        webhook.url,
                        resp.status()
                    );
                    return;
                }
                warn!(
                    "Webhook {} to {} returned status {}",
                    webhook.id,
                    webhook.url,
                    resp.status()
                );
            }
            Err(e) => {
                warn!("Webhook {} to {} failed: {}", webhook.id, webhook.url, e);
            }
        }
    }

    error!(
        "Webhook {} to {} failed after {} retries",
        webhook.id,
        webhook.url,
        max_retries + 1
    );
}

/// Extract the event type name from a HubEvent for webhook filtering.
fn event_type_name(event: &HubEvent) -> &'static str {
    match event {
        HubEvent::Chat { .. } => "Chat",
        HubEvent::UserJoin { .. } => "UserJoin",
        HubEvent::UserQuit { .. } => "UserQuit",
        HubEvent::UserInfo { .. } => "UserInfo",
        HubEvent::HubName { .. } => "HubName",
        HubEvent::OpListUpdate { .. } => "OpListUpdate",
        HubEvent::Kick { .. } => "Kick",
        HubEvent::GatewayStatus { .. } => "GatewayStatus",
    }
}

/// Dispatch an event to all matching webhooks.
pub async fn dispatch(manager: &WebhookManager, event: &HubEvent, webhook_config: &WebhookConfig) {
    let event_type = event_type_name(event);
    let matching = manager.get_matching(event_type).await;

    if matching.is_empty() {
        return;
    }

    let payload = match serde_json::to_string(event) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to serialize event for webhook: {}", e);
            return;
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(webhook_config.timeout_secs))
        .build()
        .unwrap_or_default();

    let retry_delay = Duration::from_secs(webhook_config.retry_delay_secs);
    let max_retries = webhook_config.max_retries;

    for webhook in &matching {
        let client = client.clone();
        let webhook = webhook.clone();
        let payload = payload.clone();
        tokio::spawn(async move {
            deliver_one(&client, &webhook, &payload, max_retries, retry_delay).await;
        });
    }
}

/// Run the webhook dispatcher loop, consuming events from the event bus.
pub async fn run_dispatcher(
    manager: Arc<WebhookManager>,
    mut event_rx: tokio::sync::broadcast::Receiver<HubEvent>,
    webhook_config: WebhookConfig,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                dispatch(&manager, &event, &webhook_config).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!("Webhook dispatcher lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                info!("Webhook dispatcher shutting down (bus closed)");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_payload_nonempty() {
        let sig = sign_payload("my-secret", b"test payload");
        assert!(!sig.is_empty());
        // HMAC-SHA256 produces a 64-char hex string
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn test_sign_payload_empty_secret() {
        let sig = sign_payload("", b"test payload");
        assert!(sig.is_empty());
    }

    #[test]
    fn test_sign_payload_deterministic() {
        let sig1 = sign_payload("secret", b"data");
        let sig2 = sign_payload("secret", b"data");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_sign_payload_different_secrets() {
        let sig1 = sign_payload("secret1", b"data");
        let sig2 = sign_payload("secret2", b"data");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_event_type_name() {
        let event = HubEvent::Chat {
            nick: "test".to_string(),
            message: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(event_type_name(&event), "Chat");

        let event = HubEvent::UserJoin {
            nick: "test".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(event_type_name(&event), "UserJoin");

        let event = HubEvent::Kick {
            nick: "test".to_string(),
            by: "admin".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(event_type_name(&event), "Kick");
    }
}
