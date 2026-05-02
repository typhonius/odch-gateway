//! Built-in bot command engine.
//!
//! Gateway processes chat messages starting with "!" and dispatches to
//! registered command handlers. Response delivery matches the v3 bot's
//! message model: public chat, single-user chat, bot PM, hub PM, or raw
//! protocol commands.
//!
//! Fun commands (coin, roll, 8ball, etc.) are no longer built-in. External
//! bots register via the Bot API, which creates virtual users on the hub.
//! Bots poll for commands and respond via the chat/PM API endpoints.

pub mod commands;

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{mpsc, RwLock};

/// Context passed to every command handler.
pub struct CommandContext {
    pub nick: String,
    pub args: String,
    pub db: PgPool,
    pub hub_tx: mpsc::Sender<String>,
    pub bot_registry: Arc<crate::state::BotRegistry>,
    pub hub_state: Arc<crate::state::HubState>,
    pub event_bus: Arc<crate::bus::EventBus>,
}

/// Response from a command handler.
#[allow(dead_code)]
pub enum CommandResponse {
    /// `<nick> message` broadcast to ALL users in main chat (v3 PUBLIC_ALL).
    /// Used for: seen, first, quote, kick/ban/gag announcements, ungag, unban.
    ChatAll(String),
    /// `<nick> message` to ONLY the requesting user in main chat (v3 PUBLIC_SINGLE).
    /// Used for: help, commands, stats, history, search, last, watch, unwatch, info.
    ChatSingle(String),
    /// PM from bot nick to user (v3 BOT_PM).
    /// Used for: tell confirmation, gag notice to victim.
    BotPm(String),
    /// PM from Hub-Security to user (v3 HUB_PM).
    /// Used for: kick reason sent to victim (the ONLY thing that should be Hub-Security PM).
    HubPm(String),
    /// Raw `$HubName` protocol string to all users (v3 HUB_PUBLIC).
    /// Used for: topic.
    HubTopic(String),
    /// Multiple responses (some commands need to send several messages).
    Multi(Vec<CommandResponse>),
}

/// A command handler function.
pub type CommandHandler = Box<
    dyn Fn(
            CommandContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CommandResponse> + Send>>
        + Send
        + Sync,
>;

/// The command registry.
pub struct CommandEngine {
    commands: HashMap<String, CommandHandler>,
    aliases: HashMap<String, String>,
    /// Commands that an external bot has claimed (disabled for built-in handling).
    disabled: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Hub state — used to read hub_name for response nick.
    hub_state: Arc<crate::state::HubState>,
}

impl CommandEngine {
    pub fn new(hub_state: Arc<crate::state::HubState>) -> Self {
        let mut engine = Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
            disabled: Arc::new(RwLock::new(std::collections::HashSet::new())),
            hub_state,
        };
        commands::register_all(&mut engine);
        engine
    }

    /// Register a command handler.
    pub fn register(&mut self, name: &str, aliases: &[&str], handler: CommandHandler) {
        self.commands.insert(name.to_string(), handler);
        for alias in aliases {
            self.aliases.insert(alias.to_string(), name.to_string());
        }
    }

    /// Disable built-in handling for commands that Dragon handles.
    pub async fn disable_commands(&self, names: &[String]) {
        let mut disabled = self.disabled.write().await;
        for name in names {
            disabled.insert(name.to_lowercase());
        }
    }

    /// Re-enable all built-in commands (when Dragon disconnects).
    pub async fn enable_all(&self) {
        self.disabled.write().await.clear();
    }

    /// Handle a chat message: dispatch to built-in handler or route to external bot.
    /// Returns None only if the message isn't a command at all.
    pub async fn try_handle(
        &self,
        nick: &str,
        message: &str,
        db: PgPool,
        hub_tx: mpsc::Sender<String>,
        bot_registry: Arc<crate::state::BotRegistry>,
        event_bus: Arc<crate::bus::EventBus>,
    ) -> Option<CommandResponse> {
        let msg = message.trim();
        if !msg.starts_with('!') {
            return None;
        }

        let without_prefix = &msg[1..];
        let (cmd_name, args) = match without_prefix.find(' ') {
            Some(pos) => (&without_prefix[..pos], without_prefix[pos + 1..].trim()),
            None => (without_prefix, ""),
        };

        let cmd_lower = cmd_name.to_lowercase();

        // If an external bot claims this command, route to it directly
        if self.disabled.read().await.contains(&cmd_lower) {
            let bots = bot_registry.bots.read().await;
            for bot in bots.values() {
                if bot.commands.contains(&cmd_lower) {
                    let _ = bot.event_tx.send(crate::state::BotEvent::Command {
                        from_nick: nick.to_string(),
                        command: cmd_lower,
                        args: args.to_string(),
                        timestamp: chrono::Utc::now(),
                    });
                    break;
                }
            }
            return None;
        }

        // Resolve alias
        let resolved = self
            .aliases
            .get(&cmd_lower)
            .cloned()
            .unwrap_or_else(|| cmd_lower.clone());

        let handler = self.commands.get(&resolved)?;

        let ctx = CommandContext {
            nick: nick.to_string(),
            args: args.to_string(),
            db,
            hub_tx,
            bot_registry,
            hub_state: self.hub_state.clone(),
            event_bus,
        };

        Some(handler(ctx).await)
    }

    /// Send a command response back to the hub.
    pub async fn send_response(
        &self,
        response: CommandResponse,
        nick: &str,
        hub_tx: &mpsc::Sender<String>,
    ) {
        match response {
            CommandResponse::Multi(responses) => {
                for r in responses {
                    self.send_response_single(r, nick, hub_tx).await;
                }
            }
            other => self.send_response_single(other, nick, hub_tx).await,
        }
    }

    /// Send a single (non-Multi) command response back to the hub.
    async fn send_response_single(
        &self,
        response: CommandResponse,
        nick: &str,
        hub_tx: &mpsc::Sender<String>,
    ) {
        // Use the hub name as the system identity for chat responses
        let hub_name = self.hub_state.hub_name.read().await;
        let system_nick = if hub_name.is_empty() {
            "Hub".to_string()
        } else {
            hub_name.clone()
        };
        drop(hub_name);

        match response {
            CommandResponse::ChatAll(msg) => {
                let cmd = serde_json::json!({
                    "type": "send_chat_as",
                    "nick": system_nick,
                    "message": msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            CommandResponse::ChatSingle(msg) => {
                let cmd = serde_json::json!({
                    "type": "send_to_as",
                    "nick": system_nick,
                    "to": nick,
                    "message": msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            CommandResponse::BotPm(msg) => {
                let cmd = serde_json::json!({
                    "type": "send_pm_as",
                    "from": "Hub-Security",
                    "to": nick,
                    "message": msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            CommandResponse::HubPm(msg) => {
                let cmd = serde_json::json!({
                    "type": "send_to",
                    "nick": nick,
                    "message": msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            CommandResponse::HubTopic(topic) => {
                let cmd = serde_json::json!({
                    "type": "send_all",
                    "message": topic,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
            CommandResponse::Multi(_) => {
                // Multi is handled by send_response; should not reach here.
            }
        }
    }
}
