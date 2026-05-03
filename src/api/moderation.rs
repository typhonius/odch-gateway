use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::db::queries;
use crate::error::AppError;
use crate::event::HubEvent;
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
    /// Ban duration in seconds. Omit for permanent.
    #[serde(default)]
    pub duration_secs: Option<i64>,
}

#[derive(Deserialize)]
pub struct GagRequest {
    #[serde(default)]
    pub reason: String,
    /// Gag duration in seconds. Omit for permanent.
    #[serde(default)]
    pub duration_secs: Option<i64>,
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

    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    let ban_nick = if body.ip.is_some() { None } else { Some(nick.as_str()) };
    let ban_ip = body.ip.as_deref();
    let expires_at = body.duration_secs.map(|s| chrono::Utc::now() + chrono::Duration::seconds(s));
    queries::create_ban(pool.inner(), ban_nick, ban_ip, &body.reason, "api", expires_at).await?;

    // Kick the user if they're online (NMDC protocol operation)
    if state.hub_state.users.read().await.contains_key(&nick) {
        send_hub_command(&state, serde_json::json!({"type": "kick", "nick": nick})).await?;
    }

    state.event_bus.publish(HubEvent::Ban {
        nick: nick.clone(),
        by: "api".to_string(),
        reason: body.reason.clone(),
        timestamp: chrono::Utc::now(),
    });

    Ok(Json(serde_json::json!({
        "status": "banned",
        "nick": nick,
        "reason": body.reason,
    })))
}

/// DELETE /api/users/:nick/ban
pub async fn unban_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    match queries::check_ban(pool.inner(), &nick).await? {
        Some(ban) => {
            queries::delete_ban(pool.inner(), ban.id).await?;
        }
        None => {
            return Err(AppError::NotFound(format!("{} is not banned", nick)));
        }
    }

    state.event_bus.publish(HubEvent::Unban {
        nick: nick.clone(),
        by: "api".to_string(),
        timestamp: chrono::Utc::now(),
    });

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

    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    let expires_at = body.duration_secs.map(|s| chrono::Utc::now() + chrono::Duration::seconds(s));
    queries::create_gag(pool.inner(), &nick, &body.reason, "api", expires_at).await?;

    state.event_bus.publish(HubEvent::Gag {
        nick: nick.clone(),
        by: "api".to_string(),
        reason: body.reason.clone(),
        timestamp: chrono::Utc::now(),
    });

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

    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    match queries::check_gag(pool.inner(), &nick).await? {
        Some(gag) => {
            queries::delete_gag(pool.inner(), gag.id).await?;
        }
        None => {
            return Err(AppError::NotFound(format!("{} is not gagged", nick)));
        }
    }

    state.event_bus.publish(HubEvent::Ungag {
        nick: nick.clone(),
        by: "api".to_string(),
        timestamp: chrono::Utc::now(),
    });

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

/// GET /api/v1/bans
pub async fn list_bans(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;
    let bans = queries::list_bans(pool.inner()).await?;
    Ok(Json(serde_json::json!({
        "bans": bans,
        "count": bans.len(),
    })))
}

/// GET /api/v1/gags
pub async fn list_gags(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;
    let gags = queries::list_gags(pool.inner()).await?;
    Ok(Json(serde_json::json!({
        "gags": gags,
        "count": gags.len(),
    })))
}

/// GET /api/v1/users/registered
pub async fn list_registered_users(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;
    let users = queries::list_registered_users(pool.inner()).await?;
    Ok(Json(serde_json::json!({
        "users": users,
        "count": users.len(),
    })))
}

/// GET /api/v1/hub/topic
pub async fn get_topic(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let topic = state.hub_state.topic.read().await.clone();
    Ok(Json(serde_json::json!({ "topic": topic })))
}

#[derive(Deserialize)]
pub struct SetTopicRequest {
    pub topic: String,
}

/// PUT /api/v1/hub/topic
pub async fn set_topic(
    State(state): State<AppState>,
    Json(body): Json<SetTopicRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Update in-memory state
    *state.hub_state.topic.write().await = body.topic.clone();

    // Persist to DB if available
    if let Some(pool) = state.db_pool.as_ref() {
        let _ = queries::set_setting(pool.inner(), "topic", &body.topic).await;
    }

    // Broadcast to hub
    send_hub_command(
        &state,
        serde_json::json!({"type": "set_topic", "topic": body.topic}),
    )
    .await?;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "topic": body.topic,
    })))
}

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    pub password: String,
    #[serde(default = "default_password_permission")]
    pub permission: Option<i16>,
}

fn default_password_permission() -> Option<i16> {
    None
}

/// PUT /api/v1/users/:nick/password
pub async fn set_password(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<SetPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_nick(&nick)?;

    if body.password.is_empty() {
        return Err(AppError::BadRequest("Password cannot be empty".to_string()));
    }

    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".into()))?;

    let password_hash = bcrypt::hash(&body.password, 10)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    // Upsert: create user if they don't exist, update password if they do
    let permission = body.permission.unwrap_or(1); // default to registered
    sqlx::query(
        "INSERT INTO users (nick, password_hash, permission) VALUES ($1, $2, $3) \
         ON CONFLICT (nick) DO UPDATE SET password_hash = EXCLUDED.password_hash, \
         permission = CASE WHEN $3 > 0 THEN $3 ELSE users.permission END",
    )
    .bind(&nick)
    .bind(&password_hash)
    .bind(permission)
    .execute(pool.inner())
    .await?;

    Ok(Json(serde_json::json!({
        "status": "password_set",
        "nick": nick,
    })))
}
