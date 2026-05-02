//! Connection greeter that sends hub topic and welcome message to newly joined users.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::bus::EventBus;
use crate::config::GreetingConfig;
use crate::event::HubEvent;
use crate::state::HubState;

const DEFAULT_WELCOME: &str = "Welcome to {hub_name}. Type !help for commands.";

/// Run the connection greeter loop. Should be spawned as a tokio task.
pub async fn run(
    event_bus: Arc<EventBus>,
    hub_state: Arc<HubState>,
    hub_tx: mpsc::Sender<String>,
    config: Option<GreetingConfig>,
) {
    let mut rx = event_bus.subscribe();
    tracing::info!("Connection greeter started");

    let welcome_template = config
        .and_then(|c| c.welcome_message)
        .unwrap_or_else(|| DEFAULT_WELCOME.to_string());

    loop {
        match rx.recv().await {
            Ok(HubEvent::UserJoin { ref nick, .. }) => {
                let hub_name = hub_state.hub_name.read().await.clone();
                let topic = hub_state.topic.read().await.clone();

                // Send hub topic
                let hub_name_msg = if topic.is_empty() {
                    format!("$HubName {}|", hub_name)
                } else {
                    format!("$HubName {} - {}|", hub_name, topic)
                };
                let cmd = serde_json::json!({
                    "type": "send_raw_to",
                    "nick": nick,
                    "data": hub_name_msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;

                // Send welcome message
                let welcome = welcome_template.replace("{hub_name}", &hub_name);
                let welcome_msg = format!("<Hub-Security> {}|", welcome);
                let cmd = serde_json::json!({
                    "type": "send_raw_to",
                    "nick": nick,
                    "data": welcome_msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Connection greeter lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!("Connection greeter shutting down (bus closed)");
                break;
            }
        }
    }
}
