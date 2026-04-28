use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::bus::EventBus;
use crate::config::AdminConfig;
use crate::event::HubEvent;
use crate::nmdc::protocol::{self, NmdcMessage};
use crate::state::HubState;

/// Default initial reconnect delay in seconds.
const DEFAULT_RECONNECT_DELAY: u64 = 5;
/// Maximum reconnect delay in seconds (cap for exponential backoff).
const MAX_RECONNECT_DELAY: u64 = 300;
/// Timeout for reading the initial auth response.
const AUTH_TIMEOUT_SECS: u64 = 10;

/// Run the admin port client loop with auto-reconnect.
///
/// The admin client connects to the OpenDCHub admin port, authenticates with
/// the configured password, enables the admin event stream, and populates
/// initial hub state via `$GetStatus` and `$GetUserList`.
///
/// Commands can be sent to the admin port through the returned `mpsc::Sender`.
/// For example: `"$Kick nick|"`, `"$GetStatus|"`, `"$AddBanEntry ip|"`.
///
/// This function runs forever (reconnecting on failure) and should be spawned
/// into a tokio task.
pub async fn run(
    config: AdminConfig,
    event_bus: Arc<EventBus>,
    hub_state: Arc<HubState>,
    mut cmd_rx: mpsc::Receiver<String>,
) {
    let mut delay = DEFAULT_RECONNECT_DELAY;

    loop {
        info!(
            "Admin client connecting to {}:{}...",
            config.host, config.port
        );

        match connect_and_run(&config, &event_bus, &hub_state, &mut cmd_rx).await {
            Ok(()) => {
                // Clean disconnect: reset backoff so the next failure starts fresh
                delay = DEFAULT_RECONNECT_DELAY;
                info!("Admin port disconnected cleanly");
            }
            Err(e) => {
                error!("Admin port connection error: {}", e);
            }
        }

        *hub_state.connected.write().await = false;
        event_bus.publish(HubEvent::GatewayStatus {
            connected: false,
            message: format!("Admin port disconnected. Reconnecting in {}s...", delay),
            timestamp: chrono::Utc::now(),
        });

        info!("Admin client reconnecting in {}s...", delay);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

        // Exponential backoff capped at MAX_RECONNECT_DELAY
        delay = (delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn connect_and_run(
    config: &AdminConfig,
    event_bus: &Arc<EventBus>,
    hub_state: &Arc<HubState>,
    cmd_rx: &mut mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{}:{}", config.host, config.port);
    let mut stream = TcpStream::connect(&addr).await?;
    let mut buf = vec![0u8; 65536];
    let mut partial = String::new();

    // ---- Phase 1: Read welcome banner, then authenticate ----
    // The admin port sends a welcome banner on connect, then expects
    // "$AdminPass <password>|" as the authentication command.
    let timeout = tokio::time::Duration::from_secs(AUTH_TIMEOUT_SECS);

    // Read the welcome banner first
    let n = tokio::time::timeout(timeout, stream.read(&mut buf)).await??;
    if n == 0 {
        return Err("Admin port closed connection before sending welcome".into());
    }
    let welcome = String::from_utf8_lossy(&buf[..n]);
    info!("Admin port welcome: {}", welcome.trim());

    // Send authentication command
    let auth_cmd = format!("$AdminPass {}|", config.password);
    stream.write_all(auth_cmd.as_bytes()).await?;

    // Read the auth response — expect "Password accepted" on success
    let n = tokio::time::timeout(timeout, stream.read(&mut buf)).await??;
    if n == 0 {
        return Err("Admin port closed connection (auth rejected?)".into());
    }

    let auth_response = String::from_utf8_lossy(&buf[..n]);
    if auth_response.contains("Bad Admin Password") {
        return Err("Admin port authentication failed: bad password".into());
    }
    if auth_response.contains("already logged in") {
        return Err("Admin port authentication failed: administrator already logged in".into());
    }

    // Process any messages in the auth response
    partial.push_str(&auth_response);
    let (messages, remainder) = protocol::split_admin_messages(&partial);
    partial = remainder;

    for raw in &messages {
        let msg = protocol::parse_message(raw);
        info!("Admin auth response: {:?}", msg);
    }

    info!(
        "Authenticated to admin port at {}:{}",
        config.host, config.port
    );

    // Mark hub as connected
    *hub_state.connected.write().await = true;
    event_bus.publish(HubEvent::GatewayStatus {
        connected: true,
        message: "Connected to hub".to_string(),
        timestamp: chrono::Utc::now(),
    });

    // ---- Phase 2: Enable events and request initial state ----
    let setup = "$Set admin_events 1|$GetStatus|$GetUserList|";
    stream.write_all(setup.as_bytes()).await?;

    // Schedule a delayed re-request for the user list — SCRIPT users may not
    // have registered by the time the initial $GetUserList runs.
    let mut refresh_delay = tokio::time::interval(std::time::Duration::from_secs(10));
    refresh_delay.tick().await; // skip immediate tick
    let mut refreshed = false;

    // ---- Phase 3: Main read loop ----
    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    info!("Admin port connection closed by server");
                    return Ok(());
                }

                partial.push_str(&String::from_utf8_lossy(&buf[..n]));

                if partial.len() > 1_048_576 {
                    warn!("Admin partial buffer exceeded 1MB, discarding");
                    partial.clear();
                    continue;
                }

                let (messages, remainder) = protocol::split_admin_messages(&partial);
                partial = remainder;

                for raw in &messages {
                    let msg = protocol::parse_message(raw);
                    handle_admin_message(msg, event_bus, hub_state).await;
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(command) => {
                        stream.write_all(command.as_bytes()).await?;
                    }
                    None => {
                        info!("Admin command channel closed, disconnecting");
                        return Ok(());
                    }
                }
            }
            _ = refresh_delay.tick(), if !refreshed => {
                // Re-request user list after scripts have had time to register
                info!("Refreshing user list (delayed)");
                stream.write_all(b"$GetUserList|").await?;
                refreshed = true;
            }
        }
    }
}

/// Handle a parsed message from the admin port.
///
/// The admin port sends three kinds of messages:
/// - `$Event TYPE data|` -- real-time event stream (when admin_events is enabled)
/// - `STATUS key|value|` -- response lines from `$GetStatus`
/// - `USER nick|ip|share|type|desc|email|speed|` -- response lines from `$GetUserList`
async fn handle_admin_message(msg: NmdcMessage, event_bus: &EventBus, hub_state: &HubState) {
    let now = chrono::Utc::now;

    match msg {
        // ---- Real-time event stream ----
        NmdcMessage::Event { event_type, data } => {
            match event_type.as_str() {
                "JOIN" => {
                    event_bus.publish(HubEvent::UserJoin {
                        nick: data,
                        timestamp: now(),
                    });
                }
                "QUIT" => {
                    hub_state.users.write().await.remove(&data);
                    event_bus.publish(HubEvent::UserQuit {
                        nick: data,
                        timestamp: now(),
                    });
                }
                "CHAT" => {
                    // Format: "nick message text"
                    if let Some(space) = data.find(' ') {
                        event_bus.publish(HubEvent::Chat {
                            nick: data[..space].to_string(),
                            message: data[space + 1..].to_string(),
                            timestamp: now(),
                        });
                    }
                }
                "KICK" => {
                    // Format: "victim kicker"
                    let parts: Vec<&str> = data.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        event_bus.publish(HubEvent::Kick {
                            nick: parts[0].to_string(),
                            by: parts[1].to_string(),
                            timestamp: now(),
                        });
                    }
                }
                "MYINFO" => {
                    // The hub strips "$MyINFO $ALL " before emitting the event,
                    // so data starts with the nick. Re-add the prefix so the
                    // standard protocol parser can handle it.
                    let synthetic = format!("$MyINFO $ALL {}|", data);
                    if let NmdcMessage::MyInfo {
                        nick,
                        description,
                        speed,
                        email,
                        share,
                    } = protocol::parse_message(&synthetic)
                    {
                        let is_op = hub_state.ops.read().await.contains(&nick);
                        hub_state.users.write().await.insert(
                            nick.clone(),
                            crate::state::HubUser {
                                nick: nick.clone(),
                                description: description.clone(),
                                speed: speed.clone(),
                                email: email.clone(),
                                share,
                                is_op,
                            },
                        );
                        event_bus.publish(HubEvent::UserInfo {
                            nick,
                            description,
                            speed,
                            email,
                            share,
                            timestamp: now(),
                        });
                    }
                }
                "SEARCH" => {
                    // Search events are informational only; no hub event type for them.
                }
                other => {
                    warn!("Unhandled admin event type: {} data={}", other, data);
                }
            }
        }

        // ---- $GetStatus responses ----
        NmdcMessage::Status { key, value } => match key.as_str() {
            "hub_name" => {
                *hub_state.hub_name.write().await = value.clone();
                event_bus.publish(HubEvent::HubName {
                    name: value,
                    timestamp: now(),
                });
            }
            "total_share" => {
                if let Ok(share) = value.parse::<u64>() {
                    *hub_state.total_share.write().await = share;
                }
            }
            "uptime" => {
                if let Ok(secs) = value.parse::<u64>() {
                    *hub_state.uptime_secs.write().await = secs;
                }
            }
            "hub_port" => {
                if let Ok(port) = value.parse::<u16>() {
                    *hub_state.hub_port.write().await = port;
                }
            }
            "tls_port" => {
                if let Ok(port) = value.parse::<u16>() {
                    *hub_state.tls_port.write().await = port;
                }
            }
            "max_users" => {
                if let Ok(max) = value.parse::<u32>() {
                    *hub_state.max_users.write().await = max;
                }
            }
            _ => {
                info!("Admin status: {}={}", key, value);
            }
        },

        // ---- $GetUserList responses ----
        NmdcMessage::UserEntry {
            nick,
            ip: _,
            share,
            user_type,
            description,
            email,
            speed,
        } => {
            if nick.is_empty() {
                return;
            }
            // ADMIN type = admin port session (not a real user), skip it
            if user_type == "ADMIN" {
                return;
            }

            let share_bytes = share.parse::<u64>().unwrap_or(0);
            let is_op = matches!(
                user_type.as_str(),
                "OP" | "OP_ADMIN" | "ADMIN" | "1" | "2"
            );

            hub_state.users.write().await.insert(
                nick.clone(),
                crate::state::HubUser {
                    nick: nick.clone(),
                    description: description.clone(),
                    speed: speed.clone(),
                    email: email.clone(),
                    share: share_bytes,
                    is_op,
                },
            );

            if is_op {
                let mut ops = hub_state.ops.write().await;
                if !ops.contains(&nick) {
                    ops.push(nick.clone());
                }
            }

            event_bus.publish(HubEvent::UserInfo {
                nick,
                description,
                speed,
                email,
                share: share_bytes,
                timestamp: now(),
            });
        }

        // ---- Other messages (chat, hub name, etc. sent as standard NMDC) ----
        NmdcMessage::HubName { name } => {
            *hub_state.hub_name.write().await = name.clone();
            event_bus.publish(HubEvent::HubName {
                name,
                timestamp: now(),
            });
        }
        NmdcMessage::Chat { nick, message } => {
            event_bus.publish(HubEvent::Chat {
                nick,
                message,
                timestamp: now(),
            });
        }

        NmdcMessage::Unknown(raw) => {
            // Admin port may send informational text lines; log them
            if !raw.is_empty() {
                info!("Admin port unhandled message: {}", raw);
            }
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::EventBus;
    use crate::state::HubState;

    /// Verify that handle_admin_message correctly processes a UserEntry
    /// and populates hub state.
    #[tokio::test]
    async fn test_handle_user_entry() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        let msg = NmdcMessage::UserEntry {
            nick: "TestUser".to_string(),
            ip: "192.168.1.1".to_string(),
            share: "12345".to_string(),
            user_type: "0".to_string(),
            description: "test desc".to_string(),
            email: "test@example.com".to_string(),
            speed: "LAN(T1)".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        // Verify user was inserted into state
        let users = state.users.read().await;
        assert!(users.contains_key("TestUser"));
        let user = &users["TestUser"];
        assert_eq!(user.share, 12345);
        assert!(!user.is_op);

        // Verify event was published
        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::UserInfo { nick, share, .. } => {
                assert_eq!(nick, "TestUser");
                assert_eq!(share, 12345);
            }
            _ => panic!("Expected UserInfo event, got {:?}", event),
        }
    }

    /// Verify that an op user_type is correctly identified.
    #[tokio::test]
    async fn test_handle_op_user_entry() {
        let bus = EventBus::new(16);
        let state = HubState::new();

        let msg = NmdcMessage::UserEntry {
            nick: "Admin".to_string(),
            ip: "10.0.0.1".to_string(),
            share: "999".to_string(),
            user_type: "1".to_string(),
            description: "admin".to_string(),
            email: "".to_string(),
            speed: "LAN(T1)".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        let users = state.users.read().await;
        assert!(users["Admin"].is_op);

        let ops = state.ops.read().await;
        assert!(ops.contains(&"Admin".to_string()));
    }

    /// Verify that STATUS hub_name updates hub state.
    #[tokio::test]
    async fn test_handle_status_hub_name() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        let msg = NmdcMessage::Status {
            key: "hub_name".to_string(),
            value: "MyHub".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        assert_eq!(*state.hub_name.read().await, "MyHub");

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::HubName { name, .. } => assert_eq!(name, "MyHub"),
            _ => panic!("Expected HubName event"),
        }
    }

    /// Verify that STATUS total_share updates hub state.
    #[tokio::test]
    async fn test_handle_status_total_share() {
        let bus = EventBus::new(16);
        let state = HubState::new();

        let msg = NmdcMessage::Status {
            key: "total_share".to_string(),
            value: "1073741824".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        assert_eq!(*state.total_share.read().await, 1073741824);
    }

    /// Verify that Event JOIN is correctly published.
    #[tokio::test]
    async fn test_handle_event_join() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        let msg = NmdcMessage::Event {
            event_type: "JOIN".to_string(),
            data: "NewUser".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::UserJoin { nick, .. } => assert_eq!(nick, "NewUser"),
            _ => panic!("Expected UserJoin event"),
        }
    }

    /// Verify that Event QUIT removes user from state and publishes event.
    #[tokio::test]
    async fn test_handle_event_quit() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        // Pre-populate the user
        state.users.write().await.insert(
            "LeavingUser".to_string(),
            crate::state::HubUser {
                nick: "LeavingUser".to_string(),
                description: String::new(),
                speed: String::new(),
                email: String::new(),
                share: 0,
                is_op: false,
            },
        );

        let msg = NmdcMessage::Event {
            event_type: "QUIT".to_string(),
            data: "LeavingUser".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        assert!(!state.users.read().await.contains_key("LeavingUser"));

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::UserQuit { nick, .. } => assert_eq!(nick, "LeavingUser"),
            _ => panic!("Expected UserQuit event"),
        }
    }

    /// Verify that Event CHAT is parsed and published correctly.
    #[tokio::test]
    async fn test_handle_event_chat() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        let msg = NmdcMessage::Event {
            event_type: "CHAT".to_string(),
            data: "Alice Hello everyone!".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::Chat { nick, message, .. } => {
                assert_eq!(nick, "Alice");
                assert_eq!(message, "Hello everyone!");
            }
            _ => panic!("Expected Chat event"),
        }
    }

    /// Verify that Event KICK is parsed and published correctly.
    #[tokio::test]
    async fn test_handle_event_kick() {
        let bus = EventBus::new(16);
        let state = HubState::new();
        let mut rx = bus.subscribe();

        let msg = NmdcMessage::Event {
            event_type: "KICK".to_string(),
            data: "BadUser Admin".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::Kick { nick, by, .. } => {
                assert_eq!(nick, "BadUser");
                assert_eq!(by, "Admin");
            }
            _ => panic!("Expected Kick event"),
        }
    }

    /// Verify that empty nick UserEntry is skipped.
    #[tokio::test]
    async fn test_handle_empty_nick_user_entry() {
        let bus = EventBus::new(16);
        let state = HubState::new();

        let msg = NmdcMessage::UserEntry {
            nick: "".to_string(),
            ip: "".to_string(),
            share: "0".to_string(),
            user_type: "0".to_string(),
            description: "".to_string(),
            email: "".to_string(),
            speed: "".to_string(),
        };

        handle_admin_message(msg, &bus, &state).await;

        assert!(state.users.read().await.is_empty());
    }
}
