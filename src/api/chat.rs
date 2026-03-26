use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

/// Sanitize user input for safe embedding in NMDC protocol commands.
/// Strips pipe (command delimiter) and dollar sign (command prefix) characters.
pub fn sanitize_nmdc(input: &str) -> String {
    input.chars().filter(|c| *c != '|' && *c != '$').collect()
}

#[derive(Deserialize)]
pub struct ChatHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// GET /api/chat/history
///
/// Returns paginated chat history from the database.
pub async fn get_chat_history(
    State(state): State<AppState>,
    Query(params): Query<ChatHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".to_string()))?;

    let limit = params.limit.clamp(1, 500);
    let offset = params.offset.max(0);

    let history = queries::get_chat_history(pool, limit, offset).await?;

    Ok(Json(serde_json::json!({
        "history": history,
        "count": history.len(),
        "limit": limit,
        "offset": offset,
    })))
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
    /// Optional nick to send as. When provided, the message is broadcast
    /// via the admin port's `$DataToAll` command so it appears as that user.
    /// Requires the admin port to be configured.
    #[serde(default)]
    pub nick: Option<String>,
}

/// POST /api/chat/message
///
/// Send a chat message to the hub via NMDC protocol.
/// Without `nick`: sends as the gateway bot user via NMDC client.
/// With `nick`: spoofs as the given user via admin port `$DataToAll`.
pub async fn send_message(
    State(state): State<AppState>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.message.trim().is_empty() {
        return Err(AppError::BadRequest("Message cannot be empty".to_string()));
    }

    // Check hub connectivity
    if !*state.hub_state.connected.read().await {
        return Err(AppError::HubDisconnected);
    }

    let safe_message = sanitize_nmdc(&body.message);

    if let Some(ref nick) = body.nick {
        // Send as another user via admin port $DataToAll
        if state.config.admin.is_none() {
            return Err(AppError::Internal(
                "Admin port not configured; cannot send as other users".to_string(),
            ));
        }
        let safe_nick = sanitize_nmdc(nick);
        // $DataToAll <nick> message|  — hub strips the $DataToAll prefix
        // and broadcasts the rest to all human clients.
        let cmd = format!("$DataToAll <{}> {}|", safe_nick, safe_message);
        state
            .admin_tx
            .send(cmd)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send chat: {}", e)))?;

        Ok(Json(serde_json::json!({
            "status": "sent",
            "nick": safe_nick,
            "message": body.message,
        })))
    } else {
        // Send as gateway bot via NMDC client
        let nick = &state.config.hub.nickname;
        let nmdc_cmd = format!("<{}> {}|", nick, safe_message);
        state
            .nmdc_tx
            .send(nmdc_cmd)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send chat: {}", e)))?;

        Ok(Json(serde_json::json!({
            "status": "sent",
            "nick": nick,
            "message": body.message,
        })))
    }
}
