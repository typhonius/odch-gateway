use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::bus::EventBus;
use crate::config::HubSocketConfig;
use crate::db::pool::DbPool;
use crate::db::queries;
use crate::event::HubEvent;
use crate::state::{HubState, HubUser};

/// Default initial reconnect delay in seconds.
const DEFAULT_RECONNECT_DELAY: u64 = 5;
/// Maximum reconnect delay in seconds (cap for exponential backoff).
const MAX_RECONNECT_DELAY: u64 = 300;

/// Run the Unix socket hub client loop with auto-reconnect.
///
/// Connects to the hub's Unix domain socket, authenticates with a shared
/// secret, and receives length-prefixed JSON events. Commands can be sent
/// back through the mpsc channel.
///
/// This function runs forever (reconnecting on failure) and should be
/// spawned into a tokio task.
pub async fn run(
    config: HubSocketConfig,
    event_bus: Arc<EventBus>,
    hub_state: Arc<HubState>,
    mut cmd_rx: mpsc::Receiver<String>,
    hub_tx: mpsc::Sender<String>,
    db_pool: Option<DbPool>,
) {
    let mut delay = DEFAULT_RECONNECT_DELAY;

    loop {
        info!("Hub socket connecting to {}...", config.socket_path);

        match connect_and_run(&config, &event_bus, &hub_state, &mut cmd_rx, &hub_tx, &db_pool)
            .await
        {
            Ok(()) => {
                delay = DEFAULT_RECONNECT_DELAY;
                info!("Hub socket disconnected cleanly");
            }
            Err(e) => {
                error!("Hub socket connection error: {}", e);
            }
        }

        *hub_state.connected.write().await = false;
        event_bus.publish(HubEvent::GatewayStatus {
            connected: false,
            message: format!("Hub socket disconnected. Reconnecting in {}s...", delay),
            timestamp: chrono::Utc::now(),
        });

        info!("Hub socket reconnecting in {}s...", delay);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        delay = (delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn connect_and_run(
    config: &HubSocketConfig,
    event_bus: &Arc<EventBus>,
    hub_state: &Arc<HubState>,
    cmd_rx: &mut mpsc::Receiver<String>,
    hub_tx: &mpsc::Sender<String>,
    db_pool: &Option<DbPool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(&config.socket_path).await?;
    info!("Hub socket connected to {}", config.socket_path);

    // Authenticate
    let auth_msg = serde_json::json!({"type": "auth", "secret": config.secret});
    send_json(&mut stream, &auth_msg.to_string()).await?;

    // Read auth response
    let auth_response = read_json(&mut stream).await?;
    let auth_obj: serde_json::Value = serde_json::from_str(&auth_response)?;
    if auth_obj.get("type").and_then(|t| t.as_str()) != Some("auth_ok") {
        return Err("Hub socket authentication failed".into());
    }
    info!("Hub socket authenticated");

    // Mark connected
    *hub_state.connected.write().await = true;
    event_bus.publish(HubEvent::GatewayStatus {
        connected: true,
        message: "Connected to hub via Unix socket".to_string(),
        timestamp: chrono::Utc::now(),
    });

    // Request initial state
    send_json(&mut stream, r#"{"type":"get_status"}"#).await?;
    send_json(&mut stream, r#"{"type":"get_user_list"}"#).await?;


    // Main event loop
    let mut read_buf = vec![0u8; 65536];
    let mut partial = Vec::new();
    let mut status_interval = interval(Duration::from_secs(30));
    status_interval.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            // Periodic status refresh
            _ = status_interval.tick() => {
                send_json(&mut stream, r#"{"type":"get_status"}"#).await.ok();
                send_json(&mut stream, r#"{"type":"get_user_list"}"#).await.ok();
            }

            // Read events from hub
            result = stream.read(&mut read_buf) => {
                match result {
                    Ok(0) => {
                        info!("Hub socket closed by remote");
                        return Ok(());
                    }
                    Ok(n) => {
                        partial.extend_from_slice(&read_buf[..n]);
                        // Process complete messages
                        while partial.len() >= 4 {
                            let msg_len = u32::from_be_bytes([
                                partial[0], partial[1], partial[2], partial[3],
                            ]) as usize;

                            if msg_len > 1_048_576 {
                                return Err("Message too large".into());
                            }

                            if partial.len() < 4 + msg_len {
                                break; // incomplete
                            }

                            let json_bytes = &partial[4..4 + msg_len];
                            if let Ok(json_str) = std::str::from_utf8(json_bytes) {
                                handle_event(json_str, event_bus, hub_state, hub_tx, db_pool).await;
                            }

                            partial.drain(..4 + msg_len);
                        }
                    }
                    Err(e) => {
                        return Err(Box::new(e));
                    }
                }
            }

            // Send commands to hub
            Some(cmd) = cmd_rx.recv() => {
                if let Err(e) = send_json(&mut stream, &cmd).await {
                    error!("Failed to send command to hub: {}", e);
                    return Err(Box::new(e));
                }
            }
        }
    }
}

/// Send a length-prefixed JSON message over the Unix socket.
async fn send_json(stream: &mut UnixStream, json: &str) -> Result<(), std::io::Error> {
    let len = json.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(json.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a single length-prefixed JSON message from the Unix socket.
async fn read_json(
    stream: &mut UnixStream,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u32::from_be_bytes(len_buf) as usize;

    if msg_len > 1_048_576 {
        return Err("Message too large".into());
    }

    let mut msg_buf = vec![0u8; msg_len];
    stream.read_exact(&mut msg_buf).await?;
    Ok(String::from_utf8(msg_buf)?)
}

/// Process a JSON event from the hub.
async fn handle_event(
    json_str: &str,
    event_bus: &Arc<EventBus>,
    hub_state: &Arc<HubState>,
    hub_tx: &mpsc::Sender<String>,
    db_pool: &Option<DbPool>,
) {
    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "Failed to parse hub event: {}: {}",
                e,
                &json_str[..json_str.len().min(200)]
            );
            return;
        }
    };

    let event_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };

    match event_type {
        "chat" => {
            let nick = value.get("nick").and_then(|v| v.as_str()).unwrap_or("");
            let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("");

            // Check if user is gagged
            let is_gagged = if let Some(pool) = db_pool {
                matches!(queries::check_gag(pool.inner(), nick).await, Ok(Some(_)))
            } else {
                false
            };

            if is_gagged {
                // Notify the user, don't broadcast or publish to event bus
                let cmd = serde_json::json!({
                    "type": "send_raw_to",
                    "nick": nick,
                    "data": "<Hub-Security> You are gagged. No talking for you.|",
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            } else {
                // Echo the chat message back to hub for broadcast
                let raw = format!("<{}> {}|", nick, message);
                let cmd = serde_json::json!({
                    "type": "send_raw",
                    "data": raw,
                });
                let _ = hub_tx.send(cmd.to_string()).await;

                // Publish to event bus for DB storage, webhooks, bot commands, etc.
                event_bus.publish(HubEvent::Chat {
                    nick: nick.to_string(),
                    message: message.to_string(),
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        "user_join" => {
            let nick = value.get("nick").and_then(|v| v.as_str()).unwrap_or("");

            event_bus.publish(HubEvent::UserJoin {
                nick: nick.to_string(),
                timestamp: chrono::Utc::now(),
            });
        }

        "user_quit" => {
            let nick = value
                .get("nick")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Remove from state
            hub_state.users.write().await.remove(&nick);

            event_bus.publish(HubEvent::UserQuit {
                nick,
                timestamp: chrono::Utc::now(),
            });
        }

        "myinfo" => {
            let nick = value
                .get("nick")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let speed = value
                .get("speed")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let email = value
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let share = value.get("share").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64;

            // Update user in state
            hub_state.users.write().await.insert(
                nick.clone(),
                HubUser {
                    nick: nick.clone(),
                    description: description.clone(),
                    speed: speed.clone(),
                    email: email.clone(),
                    share,
                    is_op: false, // updated from user_list
                },
            );

            event_bus.publish(HubEvent::UserInfo {
                nick,
                description,
                speed,
                email,
                share,
                timestamp: chrono::Utc::now(),
            });
        }

        "pm" => {
            let from = value.get("from").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let to = value.get("to").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();

            event_bus.publish(HubEvent::PrivateMessage {
                from,
                to,
                message,
                timestamp: chrono::Utc::now(),
            });
        }

        "kick" => {
            let nick = value
                .get("nick")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let by = value
                .get("by")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            event_bus.publish(HubEvent::Kick {
                nick,
                by,
                timestamp: chrono::Utc::now(),
            });
        }

        "validate_nick" => {
            let nick = value.get("nick").and_then(|v| v.as_str()).unwrap_or("");
            if nick.is_empty() { return; }

            // Check if user is registered in DB
            let is_registered = if let Some(pool) = db_pool {
                match queries::get_user(pool.inner(), nick).await {
                    Ok(Some(user)) if user.password_hash.is_some() && user.permission > 0 => true,
                    _ => false,
                }
            } else {
                false
            };

            if is_registered {
                // Tell hub to challenge for password
                let cmd = serde_json::json!({
                    "type": "send_getpass",
                    "nick": nick,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            } else {
                // Not registered — let them in as regular user
                let cmd = serde_json::json!({
                    "type": "login_user",
                    "nick": nick,
                    "permission": 0,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
        }

        "check_password" => {
            let nick = value.get("nick").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let password = value.get("password").and_then(|v| v.as_str()).unwrap_or("");
            if nick.is_empty() || password.is_empty() {
                let cmd = serde_json::json!({
                    "type": "reject_user",
                    "nick": nick,
                    "reason": "Invalid credentials",
                });
                let _ = hub_tx.send(cmd.to_string()).await;
                return;
            }

            let result = if let Some(pool) = db_pool {
                match queries::get_user_with_password(pool.inner(), &nick).await {
                    Ok(Some((hash, permission))) => {
                        // bcrypt verify (blocking — use spawn_blocking)
                        let pw = password.to_string();
                        let hash_clone = hash.clone();
                        match tokio::task::spawn_blocking(move || {
                            bcrypt::verify(pw, &hash_clone)
                        }).await {
                            Ok(Ok(true)) => Some(permission),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };

            match result {
                Some(permission) => {
                    let cmd = serde_json::json!({
                        "type": "login_user",
                        "nick": nick,
                        "permission": permission,
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                    tracing::info!("User '{}' authenticated (permission={})", nick, permission);
                }
                None => {
                    let cmd = serde_json::json!({
                        "type": "reject_user",
                        "nick": nick,
                        "reason": "Incorrect password",
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                    tracing::warn!("Failed password attempt for '{}'", nick);
                }
            }
        }

        "status" => {
            if let Some(name) = value.get("hub_name").and_then(|v| v.as_str()) {
                *hub_state.hub_name.write().await = name.to_string();
            }
            if let Some(share) = value.get("share").and_then(|v| v.as_f64()) {
                *hub_state.total_share.write().await = share as u64;
            }
            if let Some(uptime) = value.get("uptime").and_then(|v| v.as_f64()) {
                *hub_state.uptime_secs.write().await = uptime as u64;
            }
            if let Some(port) = value.get("hub_port").and_then(|v| v.as_f64()) {
                *hub_state.hub_port.write().await = port as u16;
            }
            if let Some(tls) = value.get("tls_port").and_then(|v| v.as_f64()) {
                *hub_state.tls_port.write().await = tls as u16;
            }
            if let Some(max) = value.get("max_users").and_then(|v| v.as_f64()) {
                *hub_state.max_users.write().await = max as u32;
            }

            let name = hub_state.hub_name.read().await.clone();
            event_bus.publish(HubEvent::HubName {
                name,
                timestamp: chrono::Utc::now(),
            });
        }

        "user_list" => {
            if let Some(users) = value.get("users").and_then(|v| v.as_array()) {
                let mut state_users = hub_state.users.write().await;
                let mut ops = hub_state.ops.write().await;
                state_users.clear();
                ops.clear();

                for u in users {
                    let nick = u
                        .get("nick")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if nick.is_empty() {
                        continue;
                    }

                    let user_type = u.get("type").and_then(|v| v.as_str()).unwrap_or("REGULAR");
                    let is_op = matches!(user_type, "OP" | "OP_ADMIN");

                    if is_op {
                        ops.push(nick.clone());
                    }

                    state_users.insert(
                        nick.clone(),
                        HubUser {
                            nick: nick.clone(),
                            description: u
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            speed: u
                                .get("speed")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            email: u
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            share: u.get("share").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64,
                            is_op,
                        },
                    );
                }

                event_bus.publish(HubEvent::OpListUpdate {
                    ops: ops.clone(),
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        "auth_ok" | "auth_failed" | "error" => {
            // Handled during connection setup or logged
            if event_type == "error" {
                let msg = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                warn!("Hub error: {}", msg);
            }
        }

        _ => {
            // Unknown event type, just log it
            tracing::debug!("Unknown hub event type: {}", event_type);
        }
    }
}
