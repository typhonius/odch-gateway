//! Built-in OP group chat relay.
//!
//! Registers a virtual user (default "OPChat") on the hub and relays
//! private messages between all online operators.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::bus::EventBus;
use crate::config::OpChatConfig;
use crate::event::HubEvent;
use crate::state::HubState;

/// Run the OPChat relay. Should be spawned as a tokio task.
pub async fn run(
    event_bus: Arc<EventBus>,
    hub_state: Arc<HubState>,
    hub_tx: mpsc::Sender<String>,
    config: OpChatConfig,
) {
    let nick = config.nick;
    let description = config.description;

    // Register virtual user on the hub
    let register_cmd = serde_json::json!({
        "type": "add_virtual_user",
        "nick": nick,
        "description": description,
        "email": "",
        "tag": "",
        "share": 0,
        "op": true,
    });
    let _ = hub_tx.send(register_cmd.to_string()).await;
    tracing::info!("OPChat registered as virtual user '{}'", nick);

    let mut rx = event_bus.subscribe();

    loop {
        match rx.recv().await {
            Ok(HubEvent::PrivateMessage {
                ref from,
                ref to,
                ref message,
                ..
            }) => {
                // Only handle PMs sent to our nick
                if to != &nick {
                    continue;
                }

                // Format the relay message
                let relay_msg = format!("<{}> {}", from, message);

                // Get all online ops and relay to each (except sender and self)
                let users = hub_state.users.read().await;
                for user in users.values() {
                    if !user.is_op {
                        continue;
                    }
                    if user.nick == nick || user.nick == *from {
                        continue;
                    }

                    let cmd = serde_json::json!({
                        "type": "send_pm_as",
                        "from": nick,
                        "to": user.nick,
                        "message": relay_msg,
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                }
            }
            Ok(HubEvent::GatewayStatus { connected: true, .. }) => {
                // Hub reconnected — re-register virtual user
                let _ = hub_tx.send(register_cmd.to_string()).await;
                tracing::info!("OPChat re-registered after hub reconnect");
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("OPChat lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                // Unregister virtual user on shutdown
                let cmd = serde_json::json!({
                    "type": "remove_virtual_user",
                    "nick": nick,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
                tracing::info!("OPChat shutting down");
                break;
            }
        }
    }
}
