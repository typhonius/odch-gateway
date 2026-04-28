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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<i16>,
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
pub async fn list_users(
    State(state): State<AppState>,
    Query(params): Query<UsersQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let users: Vec<_> = state
        .hub_state
        .users
        .read()
        .await
        .values()
        .cloned()
        .collect();

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
            first_seen: None,
            permission: None,
        };

        if let Some(ref pool) = state.db_pool {
            if let Ok(Some(db_user)) = queries::get_user(pool.inner(), &u.nick).await {
                online_user.first_seen = db_user.first_seen.map(|t| t.to_rfc3339());
                online_user.permission = Some(db_user.permission);
            }
        }

        result.push(online_user);
    }

    result.sort_by(|a, b| a.nick.cmp(&b.nick));

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
pub async fn get_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let users = state.hub_state.users.read().await;
    let live_user = users.get(&nick).cloned();
    drop(users);

    let db_user = match state.db_pool.as_ref() {
        Some(pool) => queries::get_user(pool.inner(), &nick).await.ok().flatten(),
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
                response["first_seen"] = serde_json::json!(db.first_seen);
                response["last_seen"] = serde_json::json!(db.last_seen);
                response["permission"] = serde_json::json!(db.permission);
            }

            Ok(Json(response))
        }
        (None, Some(db)) => Ok(Json(serde_json::json!({
            "nick": db.nick,
            "description": db.description,
            "speed": db.speed,
            "email": db.email,
            "share": db.share_size,
            "is_op": false,
            "online": false,
            "first_seen": db.first_seen,
            "last_seen": db.last_seen,
            "permission": db.permission,
        }))),
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

    let history = queries::get_user_chat_history(pool.inner(), &nick, limit, offset).await?;

    Ok(Json(serde_json::json!({
        "nick": nick,
        "history": history,
        "count": history.len(),
        "limit": limit,
        "offset": offset,
    })))
}
