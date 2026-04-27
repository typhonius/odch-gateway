use std::collections::HashSet;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Deserialize;
use tokio::time::interval;
use tracing::{info, warn};

use crate::event::HubEvent;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct WsQuery {
    /// Comma-separated list of event types to filter.
    /// If empty, all events are forwarded.
    /// Supported: chat, user_join, user_quit, user_info, hub_name, op_list, kick, gateway_status
    #[serde(default)]
    pub filter: String,
    /// Optional API key for WebSocket auth (since headers are hard in browser WS).
    #[serde(default)]
    pub api_key: String,
}

/// GET /ws
///
/// Upgrade to a WebSocket connection. Subscribes to the event bus and
/// forwards matching events to the client as JSON messages.
///
/// Query parameters:
/// - `filter`: comma-separated event types (e.g., `chat,user_join`)
/// - `api_key`: API key for authentication
pub async fn ws_handler(
    State(state): State<AppState>,
    Query(params): Query<WsQuery>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Validate: API key via query param OR valid JWT session cookie (admin UI)
    let has_api_key = !params.api_key.is_empty()
        && state.config.auth.api_keys.contains(&params.api_key);
    let has_valid_session = state.config.admin_ui.as_ref().is_some_and(|ui_config| {
        headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .map(|c| crate::admin_ui::auth::validate_session_cookie(c, ui_config))
            .unwrap_or(false)
    });
    let is_valid = has_api_key || has_valid_session;

    if !is_valid {
        // We can't return an AppError from WebSocketUpgrade, so we accept
        // the upgrade and immediately close with a reason.
        return ws.on_upgrade(|mut socket| async move {
            let _ = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 4001,
                    reason: "Unauthorized".into(),
                })))
                .await;
        });
    }

    let filters = parse_filters(&params.filter);
    let event_rx = state.event_bus.subscribe();

    ws.on_upgrade(move |socket| handle_socket(socket, event_rx, filters))
}

fn parse_filters(filter_str: &str) -> HashSet<String> {
    if filter_str.is_empty() {
        return HashSet::new();
    }
    filter_str
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn event_type_tag(event: &HubEvent) -> &'static str {
    match event {
        HubEvent::Chat { .. } => "chat",
        HubEvent::UserJoin { .. } => "user_join",
        HubEvent::UserQuit { .. } => "user_quit",
        HubEvent::UserInfo { .. } => "user_info",
        HubEvent::HubName { .. } => "hub_name",
        HubEvent::OpListUpdate { .. } => "op_list",
        HubEvent::Kick { .. } => "kick",
        HubEvent::GatewayStatus { .. } => "gateway_status",
    }
}

fn matches_filter(event: &HubEvent, filters: &HashSet<String>) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters.contains(event_type_tag(event))
}

async fn handle_socket(
    mut socket: WebSocket,
    mut event_rx: tokio::sync::broadcast::Receiver<HubEvent>,
    filters: HashSet<String>,
) {
    info!("WebSocket client connected (filters: {:?})", filters);

    let mut heartbeat = interval(Duration::from_secs(30));
    // Skip the immediate first tick
    heartbeat.tick().await;

    loop {
        tokio::select! {
            // Forward events from the bus to the WebSocket client
            event_result = event_rx.recv() => {
                match event_result {
                    Ok(event) => {
                        if !matches_filter(&event, &filters) {
                            continue;
                        }
                        match serde_json::to_string(&event) {
                            Ok(json) => {
                                if socket.send(Message::Text(json)).await.is_err() {
                                    info!("WebSocket client disconnected (send failed)");
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to serialize event: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket lagged by {} events", n);
                        // Send a notification to the client
                        let msg = serde_json::json!({
                            "type": "system",
                            "data": {"message": format!("Missed {} events (lagged)", n)}
                        });
                        let _ = socket
                            .send(Message::Text(msg.to_string()))
                            .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("WebSocket closing (event bus closed)");
                        break;
                    }
                }
            }

            // Read client messages (handle pong and close)
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket client disconnected");
                        break;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // Client responded to ping, connection is alive
                    }
                    Some(Ok(_)) => {
                        // Ignore other client messages
                    }
                    Some(Err(e)) => {
                        warn!("WebSocket receive error: {}", e);
                        break;
                    }
                }
            }

            // Heartbeat ping every 30 seconds
            _ = heartbeat.tick() => {
                if socket.send(Message::Ping(vec![])).await.is_err() {
                    info!("WebSocket client disconnected (ping failed)");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filters_empty() {
        let filters = parse_filters("");
        assert!(filters.is_empty());
    }

    #[test]
    fn test_parse_filters_single() {
        let filters = parse_filters("chat");
        assert_eq!(filters.len(), 1);
        assert!(filters.contains("chat"));
    }

    #[test]
    fn test_parse_filters_multiple() {
        let filters = parse_filters("chat,user_join,user_quit");
        assert_eq!(filters.len(), 3);
        assert!(filters.contains("chat"));
        assert!(filters.contains("user_join"));
        assert!(filters.contains("user_quit"));
    }

    #[test]
    fn test_parse_filters_with_spaces() {
        let filters = parse_filters("chat , user_join , user_quit");
        assert_eq!(filters.len(), 3);
        assert!(filters.contains("chat"));
        assert!(filters.contains("user_join"));
    }

    #[test]
    fn test_matches_filter_empty_allows_all() {
        let filters = HashSet::new();
        let event = HubEvent::Chat {
            nick: "test".to_string(),
            message: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(matches_filter(&event, &filters));
    }

    #[test]
    fn test_matches_filter_specific() {
        let mut filters = HashSet::new();
        filters.insert("chat".to_string());

        let chat_event = HubEvent::Chat {
            nick: "test".to_string(),
            message: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(matches_filter(&chat_event, &filters));

        let join_event = HubEvent::UserJoin {
            nick: "test".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(!matches_filter(&join_event, &filters));
    }

    #[test]
    fn test_event_type_tags() {
        assert_eq!(
            event_type_tag(&HubEvent::Chat {
                nick: String::new(),
                message: String::new(),
                timestamp: chrono::Utc::now(),
            }),
            "chat"
        );
        assert_eq!(
            event_type_tag(&HubEvent::UserJoin {
                nick: String::new(),
                timestamp: chrono::Utc::now(),
            }),
            "user_join"
        );
        assert_eq!(
            event_type_tag(&HubEvent::Kick {
                nick: String::new(),
                by: String::new(),
                timestamp: chrono::Utc::now(),
            }),
            "kick"
        );
    }
}
