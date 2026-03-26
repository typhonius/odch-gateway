use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use sqlx::{Column, Row};

use crate::api::chat::sanitize_nmdc;
use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

/// GET /api/commands
///
/// List available bot commands. Attempts to read from a `commands` or
/// `registry` table in the database. If the table does not exist, returns
/// a static list of known commands.
pub async fn list_commands(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Try to read commands from DB if available
    if let Some(ref pool) = state.db_pool {
        let table = if queries::table_exists(pool, "registry").await {
            Some("registry")
        } else if queries::table_exists(pool, "commands").await {
            Some("commands")
        } else {
            None
        };

        if let Some(table_name) = table {
            let sql = format!("SELECT * FROM {}", table_name);
            let rows = sqlx::query(&sql).fetch_all(pool.inner()).await?;

            let mut commands = Vec::new();
            for row in &rows {
                let mut map = serde_json::Map::new();
                for col in row.columns() {
                    let val = if let Ok(s) = row.try_get::<String, _>(col.ordinal()) {
                        serde_json::Value::String(s)
                    } else if let Ok(n) = row.try_get::<i64, _>(col.ordinal()) {
                        serde_json::Value::Number(n.into())
                    } else {
                        serde_json::Value::Null
                    };
                    map.insert(col.name().to_string(), val);
                }
                commands.push(serde_json::Value::Object(map));
            }

            return Ok(Json(serde_json::json!({
                "commands": commands,
                "source": "database",
            })));
        }
    }

    // Fallback: return a static list of common bot commands
    let commands = serde_json::json!([
        {"name": "help", "description": "Show available commands"},
        {"name": "time", "description": "Show current time"},
        {"name": "uptime", "description": "Show bot uptime"},
        {"name": "seen", "description": "Check when a user was last seen"},
        {"name": "info", "description": "Show user info"},
        {"name": "commands", "description": "List all commands"},
    ]);

    Ok(Json(serde_json::json!({
        "commands": commands,
        "source": "static",
    })))
}

#[derive(Deserialize)]
pub struct ExecuteCommandRequest {
    /// Optional arguments passed to the command.
    #[serde(default)]
    pub args: String,
}

/// POST /api/commands/:name/execute
///
/// Execute a bot command by sending it as a chat message via NMDC.
/// The command is prefixed with `-` (the standard odchbot command prefix)
/// and sent as the gateway bot user.
pub async fn execute_command(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ExecuteCommandRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate command name: alphanumeric only
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(AppError::BadRequest("Invalid command name".to_string()));
    }

    if !*state.hub_state.connected.read().await {
        return Err(AppError::HubDisconnected);
    }

    let nick = &state.config.hub.nickname;
    let safe_args = sanitize_nmdc(&body.args);
    let command_text = if safe_args.is_empty() {
        format!("-{}", name)
    } else {
        format!("-{} {}", name, safe_args)
    };

    // Send as public chat
    let nmdc_cmd = format!("<{}> {}|", nick, command_text);
    state
        .nmdc_tx
        .send(nmdc_cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "executed",
        "command": name,
        "args": body.args,
    })))
}
