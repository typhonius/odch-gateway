use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

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

    let history = queries::get_chat_history(pool, limit, offset)?;

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
}

/// POST /api/chat/message
///
/// Send a chat message to the hub via NMDC protocol.
/// The message is sent as the gateway bot user.
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

    let nick = &state.config.hub.nickname;
    // NMDC public chat format: <nick> message|
    let nmdc_cmd = format!("<{}> {}|", nick, body.message);

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
