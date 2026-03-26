use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

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
        let conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;

        // Check for a registry or commands table
        let table = if has_table(&conn, "registry") {
            Some("registry")
        } else if has_table(&conn, "commands") {
            Some("commands")
        } else {
            None
        };

        if let Some(table_name) = table {
            let sql = format!("SELECT * FROM {} ORDER BY rowid", table_name);
            let mut stmt = conn.prepare(&sql)?;
            let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();

            let rows = stmt.query_map([], |row| {
                let mut map = serde_json::Map::new();
                for (i, col) in columns.iter().enumerate() {
                    let val: rusqlite::Result<String> = row.get(i);
                    map.insert(
                        col.clone(),
                        serde_json::Value::String(val.unwrap_or_default()),
                    );
                }
                Ok(serde_json::Value::Object(map))
            })?;

            let mut commands = Vec::new();
            for row in rows {
                commands.push(row?);
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

fn has_table(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1")
        .and_then(|mut s| s.query_row(rusqlite::params![name], |r| r.get::<_, i64>(0)))
        .is_ok()
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
    let command_text = if body.args.is_empty() {
        format!("-{}", name)
    } else {
        format!("-{} {}", name, body.args)
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
