use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::api::chat::sanitize_nmdc;
use crate::error::AppError;
use crate::state::AppState;

/// Validate that a nick extracted from the URL path does not contain
/// NMDC protocol control characters (`|` or `$`).
fn validate_nick(nick: &str) -> Result<(), AppError> {
    if nick.contains('|') || nick.contains('$') {
        return Err(AppError::BadRequest(
            "Nick contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct KickRequest {
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize)]
pub struct BanRequest {
    #[serde(default)]
    pub reason: String,
    /// Optional: ban by IP instead of by nick.
    #[serde(default)]
    pub ip: Option<String>,
}

#[derive(Deserialize)]
pub struct GagRequest {
    #[serde(default)]
    pub reason: String,
}

/// Helper: ensure the admin channel is available.
async fn require_admin(state: &AppState) -> Result<(), AppError> {
    if state.config.admin.is_none() {
        return Err(AppError::Internal(
            "Admin port not configured; moderation commands unavailable".to_string(),
        ));
    }
    Ok(())
}

/// POST /api/users/:nick/kick
///
/// Kick a user from the hub via the admin port.
pub async fn kick_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<KickRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;
    require_admin(&state).await?;

    // Verify user is online
    let is_online = state.hub_state.users.read().await.contains_key(&nick);
    if !is_online {
        return Err(AppError::NotFound(format!(
            "User '{}' is not currently online",
            nick
        )));
    }

    // Send $Kick via admin port
    let cmd = format!("$Kick {}|", nick);
    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send kick command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "kicked",
        "nick": nick,
        "reason": body.reason,
    })))
}

/// POST /api/users/:nick/ban
///
/// Ban a user via the admin port. Sends $AddBanEntry.
pub async fn ban_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<BanRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;
    require_admin(&state).await?;

    // If an IP is provided, ban by IP (sanitized); otherwise look up the user's nick to ban
    let ban_target = if let Some(ref ip) = body.ip {
        sanitize_nmdc(ip)
    } else {
        nick.clone()
    };

    let cmd = format!("$AddBanEntry {}|", ban_target);
    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send ban command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "banned",
        "nick": nick,
        "target": ban_target,
        "reason": body.reason,
    })))
}

/// DELETE /api/users/:nick/ban
///
/// Unban a user via the admin port. Sends $RemoveBanEntry.
pub async fn unban_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;
    require_admin(&state).await?;

    let cmd = format!("$RemoveBanEntry {}|", nick);
    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send unban command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "unbanned",
        "nick": nick,
    })))
}

/// POST /api/users/:nick/gag
///
/// Gag a user via the admin port. Sends $AddGagEntry.
pub async fn gag_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<GagRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;
    require_admin(&state).await?;

    let cmd = format!("$AddGagEntry {}|", nick);
    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send gag command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "gagged",
        "nick": nick,
        "reason": body.reason,
    })))
}

/// DELETE /api/users/:nick/gag
///
/// Ungag a user via the admin port. Sends $RemoveGagEntry.
pub async fn ungag_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;
    require_admin(&state).await?;

    let cmd = format!("$RemoveGagEntry {}|", nick);
    state
        .admin_tx
        .send(cmd)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send ungag command: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "ungagged",
        "nick": nick,
    })))
}
