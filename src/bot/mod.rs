//! Built-in bot command engine.
//!
//! Gateway processes chat messages starting with "!" and dispatches to
//! registered command handlers. These provide core hub functionality
//! (ban, tell, stats, etc.) without needing Dragon.
//!
//! When Dragon registers via the bot API, it declares which commands it
//! handles. Gateway disables its built-in handlers for those commands.

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
}

/// Response from a command handler.
pub enum CommandResponse {
    /// Send a private message to the invoking user.
    Reply(String),
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
    /// Commands that Dragon has claimed (disabled for built-in handling).
    disabled: Arc<RwLock<std::collections::HashSet<String>>>,
}

impl CommandEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
            disabled: Arc::new(RwLock::new(std::collections::HashSet::new())),
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

    /// Try to handle a chat message as a command.
    /// Returns None if the message isn't a command or the command is disabled.
    pub async fn try_handle(
        &self,
        nick: &str,
        message: &str,
        db: PgPool,
        hub_tx: mpsc::Sender<String>,
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

        // Check if disabled (Dragon is handling it)
        if self.disabled.read().await.contains(&cmd_lower) {
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
            CommandResponse::Reply(msg) => {
                let cmd = serde_json::json!({
                    "type": "send_to",
                    "nick": nick,
                    "message": msg,
                });
                let _ = hub_tx.send(cmd.to_string()).await;
            }
        }
    }
}
