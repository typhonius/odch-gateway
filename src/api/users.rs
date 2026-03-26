use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct OnlineUser {
    pub nick: String,
    pub description: String,
    pub speed: String,
    pub email: String,
    pub share: u64,
    pub is_op: bool,
    pub online: bool,
    /// Additional DB-sourced fields (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<i64>,
}

#[derive(Deserialize)]
pub struct UsersQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    100
}

/// GET /api/users
///
/// Returns online users from HubState, optionally enriched with DB data.
pub async fn list_users(
    State(state): State<AppState>,
    Query(params): Query<UsersQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Clone user data so the read lock is released before async DB queries.
    let users: Vec<_> = state.hub_state.users.read().await.values().cloned().collect();

    let mut result: Vec<OnlineUser> = Vec::new();
    for u in &users {
        let mut online_user = OnlineUser {
            nick: u.nick.clone(),
            description: u.description.clone(),
            speed: u.speed.clone(),
            email: u.email.clone(),
            share: u.share,
            is_op: u.is_op,
            online: true,
            connect_time: None,
            permissions: None,
        };

        // Enrich from DB if available
        if let Some(ref pool) = state.db_pool {
            if let Ok(Some(db_user)) = queries::get_user(pool, &u.nick).await {
                online_user.connect_time = db_user.connect_time;
                online_user.permissions = Some(db_user.permissions);
            }
        }

        result.push(online_user);
    }

    // Sort by nick for consistent ordering
    result.sort_by(|a, b| a.nick.cmp(&b.nick));

    // Apply pagination
    let offset = params.offset.max(0) as usize;
    let limit = params.limit.clamp(1, 1000) as usize;
    let total = result.len();
    let page: Vec<&OnlineUser> = result.iter().skip(offset).take(limit).collect();

    Ok(Json(serde_json::json!({
        "users": page,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/users/:nick
///
/// Returns a single user's details (live data + DB enrichment).
pub async fn get_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let users = state.hub_state.users.read().await;
    let live_user = users.get(&nick).cloned();
    drop(users);

    // Also look up DB record
    let db_user = match state.db_pool.as_ref() {
        Some(pool) => queries::get_user(pool, &nick).await.ok().flatten(),
        None => None,
    };

    match (live_user.as_ref(), &db_user) {
        (None, None) => Err(AppError::NotFound(format!("User '{}' not found", nick))),
        (Some(live), _) => {
            let mut response = serde_json::json!({
                "nick": live.nick,
                "description": live.description,
                "speed": live.speed,
                "email": live.email,
                "share": live.share,
                "is_op": live.is_op,
                "online": true,
            });

            if let Some(ref db) = db_user {
                response["connect_time"] = serde_json::json!(db.connect_time);
                response["disconnect_time"] = serde_json::json!(db.disconnect_time);
                response["permissions"] = serde_json::json!(db.permissions);
                response["ip"] = serde_json::json!(db.ip);
            }

            Ok(Json(response))
        }
        (None, Some(db)) => {
            // User not online, but found in DB
            Ok(Json(serde_json::json!({
                "nick": db.nick,
                "description": db.description,
                "speed": db.speed,
                "email": db.email,
                "share": db.share,
                "is_op": false,
                "online": false,
                "ip": db.ip,
                "connect_time": db.connect_time,
                "disconnect_time": db.disconnect_time,
                "permissions": db.permissions,
            })))
        }
    }
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_history_limit() -> i64 {
    50
}

/// GET /api/users/:nick/history
///
/// Returns the user's chat history from DB.
pub async fn get_user_history(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".to_string()))?;

    let limit = params.limit.clamp(1, 500);
    let offset = params.offset.max(0);

    let history = queries::get_user_chat_history(pool, &nick, limit, offset).await?;

    Ok(Json(serde_json::json!({
        "nick": nick,
        "history": history,
        "count": history.len(),
        "limit": limit,
        "offset": offset,
    })))
}
