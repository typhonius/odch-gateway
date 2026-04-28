use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::api::chat::sanitize_nmdc;
use crate::error::AppError;
use crate::state::AppState;

fn validate_nick(nick: &str) -> Result<(), AppError> {
    if nick.contains('|') || nick.contains('$') {
        return Err(AppError::BadRequest(
            "Nick contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

/// Send a JSON command to the hub via the admin_tx channel.
async fn send_hub_command(state: &AppState, cmd: serde_json::Value) -> Result<(), AppError> {
    state
        .admin_tx
        .send(cmd.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send hub command: {}", e)))
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
    #[serde(default)]
    pub ip: Option<String>,
}

#[derive(Deserialize)]
pub struct GagRequest {
    #[serde(default)]
    pub reason: String,
}

/// POST /api/users/:nick/kick
pub async fn kick_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<KickRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    let is_online = state.hub_state.users.read().await.contains_key(&nick);
    if !is_online {
        return Err(AppError::NotFound(format!(
            "User '{}' is not currently online",
            nick
        )));
    }

    send_hub_command(&state, serde_json::json!({"type": "kick", "nick": nick})).await?;

    Ok(Json(serde_json::json!({
        "status": "kicked",
        "nick": nick,
        "reason": body.reason,
    })))
}

/// POST /api/users/:nick/ban
pub async fn ban_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<BanRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    let ban_target = if let Some(ref ip) = body.ip {
        sanitize_nmdc(ip)
    } else {
        nick.clone()
    };

    send_hub_command(
        &state,
        serde_json::json!({"type": "ban", "entry": ban_target}),
    )
    .await?;

    Ok(Json(serde_json::json!({
        "status": "banned",
        "nick": nick,
        "target": ban_target,
        "reason": body.reason,
    })))
}

/// DELETE /api/users/:nick/ban
pub async fn unban_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    send_hub_command(&state, serde_json::json!({"type": "unban", "entry": nick})).await?;

    Ok(Json(serde_json::json!({
        "status": "unbanned",
        "nick": nick,
    })))
}

/// POST /api/users/:nick/gag
pub async fn gag_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<GagRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    send_hub_command(&state, serde_json::json!({"type": "gag", "nick": nick})).await?;

    Ok(Json(serde_json::json!({
        "status": "gagged",
        "nick": nick,
        "reason": body.reason,
    })))
}

/// DELETE /api/users/:nick/gag
pub async fn ungag_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    send_hub_command(&state, serde_json::json!({"type": "ungag", "nick": nick})).await?;

    Ok(Json(serde_json::json!({
        "status": "ungagged",
        "nick": nick,
    })))
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub nick: String,
    pub password: String,
    #[serde(default = "default_reg_type")]
    pub reg_type: u8,
}

fn default_reg_type() -> u8 {
    1
}

/// POST /api/users/register
pub async fn register_user(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&body.nick)?;

    if body.password.is_empty() {
        return Err(AppError::BadRequest("Password cannot be empty".to_string()));
    }
    if body.password.contains('|') || body.password.contains(' ') || body.password.contains('$') {
        return Err(AppError::BadRequest(
            "Password cannot contain pipe, space, or dollar characters".to_string(),
        ));
    }
    if body.reg_type > 3 {
        return Err(AppError::BadRequest(
            "reg_type must be 0-3 (0=regular, 1=registered, 2=OP, 3=admin)".to_string(),
        ));
    }

    // Store registration in gateway's database (not the hub's reglist)
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    let password_hash = bcrypt::hash(&body.password, 10)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    sqlx::query(
        "INSERT INTO users (nick, password_hash, permission) VALUES ($1, $2, $3) \
         ON CONFLICT (nick) DO UPDATE SET password_hash = EXCLUDED.password_hash, \
         permission = EXCLUDED.permission",
    )
    .bind(&body.nick)
    .bind(&password_hash)
    .bind(body.reg_type as i16)
    .execute(pool.inner())
    .await?;

    Ok(Json(serde_json::json!({
        "status": "registered",
        "nick": body.nick,
        "reg_type": body.reg_type,
    })))
}

/// DELETE /api/users/:nick/register
pub async fn unregister_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    // Clear registration from gateway database
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    sqlx::query("UPDATE users SET password_hash = NULL, permission = 0 WHERE nick = $1")
        .bind(&nick)
        .execute(pool.inner())
        .await?;

    Ok(Json(serde_json::json!({
        "status": "unregistered",
        "nick": nick,
    })))
}
