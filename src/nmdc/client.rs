use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::bus::EventBus;
use crate::config::HubConfig;
use crate::event::HubEvent;
use crate::nmdc::lock_to_key::lock_to_key;
use crate::nmdc::protocol::{self, NmdcMessage};
use crate::state::HubState;

/// Run the NMDC client loop with auto-reconnect.
pub async fn run(
    config: HubConfig,
    event_bus: Arc<EventBus>,
    hub_state: Arc<HubState>,
    mut cmd_rx: mpsc::Receiver<String>,
) {
    let mut delay = config.reconnect_delay_secs;
    let max_delay = config.max_reconnect_delay_secs;

    loop {
        info!("Connecting to hub at {}:{}...", config.host, config.port);

        match connect_and_run(&config, &event_bus, &hub_state, &mut cmd_rx).await {
            Ok(()) => {
                // Clean disconnect: reset backoff so the next failure starts fresh
                delay = config.reconnect_delay_secs;
                info!("Disconnected from hub cleanly");
            }
            Err(e) => {
                error!("Hub connection error: {}", e);
            }
        }

        // Mark disconnected
        *hub_state.connected.write().await = false;
        hub_state.users.write().await.clear();

        event_bus.publish(HubEvent::GatewayStatus {
            connected: false,
            message: format!("Disconnected. Reconnecting in {}s...", delay),
            timestamp: chrono::Utc::now(),
        });

        info!("Reconnecting in {}s...", delay);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

        // Exponential backoff
        delay = (delay * 2).min(max_delay);
    }
}

async fn connect_and_run(
    config: &HubConfig,
    event_bus: &Arc<EventBus>,
    hub_state: &Arc<HubState>,
    cmd_rx: &mut mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{}:{}", config.host, config.port);
    let mut stream = TcpStream::connect(&addr).await?;
    let mut buf = vec![0u8; 65536];
    let mut partial = String::new();

    // Phase 1: Read $Lock
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Err("Connection closed before receiving $Lock".into());
    }

    partial.push_str(&String::from_utf8_lossy(&buf[..n]));
    let (messages, remainder) = protocol::split_messages(&partial);
    partial = remainder;

    let mut got_lock = false;
    for raw in &messages {
        let msg = protocol::parse_message(raw);
        if let NmdcMessage::Lock { lock, .. } = &msg {
            let key = lock_to_key(lock);
            let key_str = String::from_utf8_lossy(&key);

            // Send handshake
            let handshake = format!(
                "$Supports UserCommand NoGetINFO NoHello UserIP2|$Key {}|$ValidateNick {}|",
                key_str, config.nickname
            );
            stream.write_all(handshake.as_bytes()).await?;
            got_lock = true;
        } else {
            // Process other messages (e.g. $HubName) that arrive with $Lock
            handle_message(msg, event_bus, hub_state).await;
        }
    }

    if !got_lock {
        return Err("No $Lock received from hub".into());
    }

    // Phase 2: Wait for $Hello or $GetPass
    let mut authenticated = false;
    let timeout = tokio::time::Duration::from_secs(10);

    loop {
        let n = tokio::time::timeout(timeout, stream.read(&mut buf)).await??;
        if n == 0 {
            return Err("Connection closed during handshake".into());
        }

        partial.push_str(&String::from_utf8_lossy(&buf[..n]));
        let (messages, remainder) = protocol::split_messages(&partial);
        partial = remainder;

        for raw in &messages {
            let msg = protocol::parse_message(raw);
            match msg {
                NmdcMessage::GetPass => {
                    if config.password.is_empty() {
                        return Err("Hub requires password but none configured".into());
                    }
                    let pass_cmd = format!("$MyPass {}|", config.password);
                    stream.write_all(pass_cmd.as_bytes()).await?;
                }
                NmdcMessage::ValidateDenide => {
                    return Err("Hub rejected our nickname".into());
                }
                NmdcMessage::Hello { ref nick } if nick == &config.nickname => {
                    authenticated = true;
                }
                other => {
                    // Process other messages (e.g. $HubName, $OpList) during handshake
                    handle_message(other, event_bus, hub_state).await;
                }
            }
        }

        if authenticated {
            break;
        }
    }

    info!("Connected to hub as {}", config.nickname);

    // Send $MyINFO — version comes from Cargo.toml at compile time,
    // not from config, so it can't be accidentally overridden.
    let myinfo = format!(
        "$MyINFO $ALL {} {}<ODCH-GW V:{},M:A,H:1/0/0,S:5>$$${}\x01${}${}$|",
        config.nickname,
        config.description,
        env!("CARGO_PKG_VERSION"),
        config.speed,
        config.email,
        config.share_size
    );
    stream.write_all(myinfo.as_bytes()).await?;

    // Mark connected
    *hub_state.connected.write().await = true;

    event_bus.publish(HubEvent::GatewayStatus {
        connected: true,
        message: "Connected to hub".to_string(),
        timestamp: chrono::Utc::now(),
    });

    // Phase 3: Main read loop
    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    return Ok(());
                }

                partial.push_str(&String::from_utf8_lossy(&buf[..n]));
                let (messages, remainder) = protocol::split_messages(&partial);
                partial = remainder;

                for raw in &messages {
                    let msg = protocol::parse_message(raw);
                    handle_message(msg, event_bus, hub_state).await;
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(command) => {
                        stream.write_all(command.as_bytes()).await?;
                    }
                    None => {
                        info!("Command channel closed, disconnecting");
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn handle_message(msg: NmdcMessage, event_bus: &EventBus, hub_state: &HubState) {
    let now = chrono::Utc::now;

    // Chat, UserJoin, UserQuit, and Kick come from the admin event stream when
    // the admin client is configured. This handler only processes messages that
    // are unique to the regular NMDC connection: user details, hub name, and op
    // list updates. Handling those here avoids duplicates on the event bus.
    match msg {
        NmdcMessage::MyInfo {
            nick,
            description,
            speed,
            email,
            share,
        } => {
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
        NmdcMessage::HubName { name } => {
            // $HubName contains "SHORT_NAME topic text" — extract the topic.
            // The admin STATUS hub_name is authoritative for the actual hub name,
            // so we only use $HubName for the topic portion.
            if let Some(space_pos) = name.find(' ') {
                let topic = name[space_pos + 1..].to_string();
                *hub_state.topic.write().await = topic;
            }
            let current = hub_state.hub_name.read().await.clone();
            if current.is_empty() {
                // Fallback: use the short name part if admin port hasn't set hub_name yet
                let short_name = name.split_whitespace().next().unwrap_or(&name).to_string();
                *hub_state.hub_name.write().await = short_name;
            }
            event_bus.publish(HubEvent::HubName {
                name,
                timestamp: now(),
            });
        }
        NmdcMessage::OpList { nicks } => {
            // Update op status on users
            let mut users = hub_state.users.write().await;
            for user in users.values_mut() {
                user.is_op = nicks.contains(&user.nick);
            }
            *hub_state.ops.write().await = nicks.clone();
            event_bus.publish(HubEvent::OpListUpdate {
                ops: nicks,
                timestamp: now(),
            });
        }
        _ => {}
    }
}
