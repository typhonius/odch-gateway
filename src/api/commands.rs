use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::api::chat::sanitize_nmdc;
use crate::error::AppError;
use crate::state::AppState;

/// GET /api/commands
///
/// List available bot commands from the bot_commands table.
pub async fn list_commands(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(ref pool) = state.db_pool {
        let rows = sqlx::query_as::<_, crate::db::models::BotCommandRecord>(
            "SELECT name, description, aliases, permission, enabled FROM bot_commands ORDER BY name",
        )
        .fetch_all(pool.inner())
        .await;

        match rows {
            Ok(commands) => {
                return Ok(Json(serde_json::json!({
                    "commands": commands,
                    "source": "database",
                })));
            }
            Err(e) => {
                tracing::warn!("Failed to query bot_commands: {}", e);
            }
        }
    }

    // Fallback: return built-in command list
    let commands = serde_json::json!([
        {"name": "ban", "description": "Ban a user"},
        {"name": "unban", "description": "Remove a ban"},
        {"name": "kick", "description": "Kick a user"},
        {"name": "gag", "description": "Mute a user"},
        {"name": "ungag", "description": "Unmute a user"},
        {"name": "tell", "description": "Leave an offline message"},
        {"name": "history", "description": "Show chat history"},
        {"name": "search", "description": "Search chat history"},
        {"name": "stats", "description": "Show hub statistics"},
        {"name": "seen", "description": "When was a user last online"},
        {"name": "first", "description": "First message by a user"},
        {"name": "last", "description": "Last message by a user"},
        {"name": "quote", "description": "Random quote"},
        {"name": "watch", "description": "Get notified when a user logs in"},
        {"name": "unwatch", "description": "Stop watching a user"},
        {"name": "info", "description": "Show user info"},
        {"name": "topic", "description": "Set hub topic"},
        {"name": "help", "description": "Show available commands"},
    ]);

    Ok(Json(serde_json::json!({
        "commands": commands,
        "source": "builtin",
    })))
}

#[derive(Deserialize)]
pub struct ExecuteCommandRequest {
    pub nick: String,
    #[serde(default)]
    pub args: String,
}

/// POST /api/commands/:name/execute
///
/// Execute a bot command by sending it as a chat message via the hub.
pub async fn execute_command(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ExecuteCommandRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(AppError::BadRequest("Invalid command name".to_string()));
    }
    if body.nick.trim().is_empty() {
        return Err(AppError::BadRequest("Nick is required".to_string()));
    }

    if !*state.hub_state.connected.read().await {
        return Err(AppError::HubDisconnected);
    }

    let safe_nick = sanitize_nmdc(&body.nick);
    let safe_args = sanitize_nmdc(&body.args);
    let command_text = if safe_args.is_empty() {
        format!("!{}", name)
    } else {
        format!("!{} {}", name, safe_args)
    };

    let cmd = serde_json::json!({"type": "send_all", "message": format!("<{}> {}", safe_nick, command_text)}).to_string();

    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "executed",
        "command": name,
        "args": body.args,
    })))
}
