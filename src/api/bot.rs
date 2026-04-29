//! Bot API endpoints — external bot platform.
//!
//! Bots register via POST /api/v1/bot/register, which creates a virtual user
//! on the hub. Bots poll for commands via GET /api/v1/bot/commands/pending
//! and respond via POST /api/v1/bot/chat or POST /api/v1/bot/pm.
//!
//! All endpoints under /api/v1/bot/ require X-API-Key authentication.

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_core::Stream;
use serde::Deserialize;
use std::convert::Infallible;

use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tells
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateTellRequest {
    pub from_nick: String,
    pub to_nick: String,
    pub message: String,
}

pub async fn create_tell(
    State(state): State<AppState>,
    Json(body): Json<CreateTellRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let id =
        queries::create_tell(pool.inner(), &body.from_nick, &body.to_nick, &body.message).await?;
    Ok(Json(serde_json::json!({"id": id, "status": "created"})))
}

pub async fn get_pending_tells(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let tells = queries::get_pending_tells(pool.inner(), &nick).await?;
    Ok(Json(
        serde_json::json!({"tells": tells, "count": tells.len()}),
    ))
}

pub async fn mark_tell_delivered(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    queries::mark_tell_delivered(pool.inner(), id).await?;
    Ok(Json(serde_json::json!({"status": "delivered"})))
}

// ---------------------------------------------------------------------------
// Bans
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateBanRequest {
    pub nick: Option<String>,
    pub ip: Option<String>,
    #[serde(default)]
    pub reason: String,
    pub banned_by: String,
    pub expires_at: Option<String>,
}

pub async fn create_ban(
    State(state): State<AppState>,
    Json(body): Json<CreateBanRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let expires = body.expires_at.as_ref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
    });
    let id = queries::create_ban(
        pool.inner(),
        body.nick.as_deref(),
        body.ip.as_deref(),
        &body.reason,
        &body.banned_by,
        expires,
    )
    .await?;
    Ok(Json(serde_json::json!({"id": id, "status": "created"})))
}

pub async fn check_ban(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let ban = queries::check_ban(pool.inner(), &nick).await?;
    Ok(Json(
        serde_json::json!({"banned": ban.is_some(), "ban": ban}),
    ))
}

pub async fn delete_ban(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let deleted = queries::delete_ban(pool.inner(), id).await?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

pub async fn get_user(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    match queries::get_user(pool.inner(), &nick).await? {
        Some(user) => Ok(Json(serde_json::json!(user))),
        None => Err(AppError::NotFound(format!("User '{}' not found", nick))),
    }
}

#[derive(Deserialize)]
pub struct UserConnectRequest {
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub tls: bool,
}

pub async fn user_connect(
    State(state): State<AppState>,
    Path(nick): Path<String>,
    Json(body): Json<UserConnectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let user_id = queries::upsert_user(pool.inner(), &nick, "", 0, "", "").await?;
    let session_id = queries::open_session(pool.inner(), user_id, &body.ip, body.tls).await?;
    let user = queries::get_user(pool.inner(), &nick).await?;
    Ok(Json(
        serde_json::json!({"user": user, "session_id": session_id}),
    ))
}

pub async fn user_disconnect(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    queries::close_sessions(pool.inner(), &nick).await?;
    queries::update_last_seen(pool.inner(), &nick).await?;
    Ok(Json(serde_json::json!({"status": "disconnected"})))
}

// ---------------------------------------------------------------------------
// Quotes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateQuoteRequest {
    pub nick: String,
    pub quote_text: String,
    pub added_by: String,
}

pub async fn create_quote(
    State(state): State<AppState>,
    Json(body): Json<CreateQuoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let id =
        queries::create_quote(pool.inner(), &body.nick, &body.quote_text, &body.added_by).await?;
    Ok(Json(serde_json::json!({"id": id, "status": "created"})))
}

#[derive(Deserialize)]
pub struct QuoteQuery {
    pub nick: Option<String>,
}

pub async fn random_quote(
    State(state): State<AppState>,
    Query(params): Query<QuoteQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let quote = queries::random_quote(pool.inner(), params.nick.as_deref()).await?;
    Ok(Json(serde_json::json!({"quote": quote})))
}

// ---------------------------------------------------------------------------
// Watches
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateWatchRequest {
    pub watcher_nick: String,
    pub watched_nick: String,
}

pub async fn create_watch(
    State(state): State<AppState>,
    Json(body): Json<CreateWatchRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    queries::create_watch(pool.inner(), &body.watcher_nick, &body.watched_nick).await?;
    Ok(Json(serde_json::json!({"status": "created"})))
}

pub async fn get_watchers(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let watchers = queries::get_watchers(pool.inner(), &nick).await?;
    Ok(Json(
        serde_json::json!({"watchers": watchers, "count": watchers.len()}),
    ))
}

pub async fn delete_watch(
    State(state): State<AppState>,
    Path((watcher, watched)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let deleted = queries::delete_watch(pool.inner(), &watcher, &watched).await?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub async fn get_current_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let stats = queries::get_stats_history(pool.inner(), 1).await?;
    Ok(Json(serde_json::json!({"stats": stats.first()})))
}

#[derive(Deserialize)]
pub struct SnapshotRequest {
    pub user_count: i32,
    pub total_share: i64,
}

pub async fn create_stats_snapshot(
    State(state): State<AppState>,
    Json(body): Json<SnapshotRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    queries::insert_stats_snapshot(pool.inner(), body.user_count, body.total_share).await?;
    Ok(Json(serde_json::json!({"status": "created"})))
}

// ---------------------------------------------------------------------------
// Chat search
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChatSearchQuery {
    pub q: String,
    pub nick: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: i64,
}

fn default_search_limit() -> i64 {
    20
}

pub async fn search_chat(
    State(state): State<AppState>,
    Query(params): Query<ChatSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let results = queries::search_chat(
        pool.inner(),
        &params.q,
        params.nick.as_deref(),
        params.limit.min(100),
    )
    .await?;
    Ok(Json(
        serde_json::json!({"results": results, "count": results.len()}),
    ))
}

pub async fn first_message(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let msg = queries::first_message(pool.inner(), &nick).await?;
    Ok(Json(serde_json::json!({"message": msg})))
}

pub async fn last_message(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let msg = queries::last_message(pool.inner(), &nick).await?;
    Ok(Json(serde_json::json!({"message": msg})))
}

// ---------------------------------------------------------------------------
// Gags
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateGagRequest {
    pub nick: String,
    #[serde(default)]
    pub reason: String,
    pub gagged_by: String,
    pub expires_at: Option<String>,
}

pub async fn create_gag(
    State(state): State<AppState>,
    Json(body): Json<CreateGagRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let expires = body.expires_at.as_ref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
    });
    let id = queries::create_gag(
        pool.inner(),
        &body.nick,
        &body.reason,
        &body.gagged_by,
        expires,
    )
    .await?;
    Ok(Json(serde_json::json!({"id": id, "status": "created"})))
}

pub async fn check_gag(
    State(state): State<AppState>,
    Path(nick): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let gag = queries::check_gag(pool.inner(), &nick).await?;
    Ok(Json(
        serde_json::json!({"gagged": gag.is_some(), "gag": gag}),
    ))
}

pub async fn delete_gag(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let deleted = queries::delete_gag(pool.inner(), id).await?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

// ---------------------------------------------------------------------------
// Bot data (key-value storage)
// ---------------------------------------------------------------------------

pub async fn list_data(
    State(state): State<AppState>,
    Path(namespace): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let entries = queries::list_bot_data(pool.inner(), &namespace).await?;
    Ok(Json(
        serde_json::json!({"entries": entries, "count": entries.len()}),
    ))
}

pub async fn get_data(
    State(state): State<AppState>,
    Path((namespace, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    match queries::get_bot_data(pool.inner(), &namespace, &key).await? {
        Some(entry) => Ok(Json(serde_json::json!(entry))),
        None => Err(AppError::NotFound(format!(
            "{}/{} not found",
            namespace, key
        ))),
    }
}

#[derive(Deserialize)]
pub struct SetDataRequest {
    pub value: String,
}

pub async fn set_data(
    State(state): State<AppState>,
    Path((namespace, key)): Path<(String, String)>,
    Json(body): Json<SetDataRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    queries::set_bot_data(pool.inner(), &namespace, &key, &body.value).await?;
    Ok(Json(serde_json::json!({"status": "set"})))
}

pub async fn delete_data(
    State(state): State<AppState>,
    Path((namespace, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("No database".into()))?;
    let deleted = queries::delete_bot_data(pool.inner(), &namespace, &key).await?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

// ---------------------------------------------------------------------------
// Bot registration
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BotRegisterRequest {
    pub nick: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub commands: Vec<String>,
}

pub async fn register_bot(
    State(state): State<AppState>,
    Json(body): Json<BotRegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Create a RegisteredBot entry in the bot registry
    {
        let mut bots = state.bot_registry.bots.write().await;
        let commands_set: std::collections::HashSet<String> =
            body.commands.iter().map(|c| c.to_lowercase()).collect();
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        bots.insert(
            body.nick.clone(),
            crate::state::RegisteredBot {
                nick: body.nick.clone(),
                description: body.description.clone(),
                email: body.email.clone(),
                tag: body.tag.clone(),
                commands: commands_set,
                event_tx,
            },
        );
    }

    // Send add_virtual_user command to hub
    let cmd = serde_json::json!({
        "type": "add_virtual_user",
        "nick": body.nick,
        "description": body.description,
        "email": body.email,
        "tag": body.tag,
        "share": 0,
        "op": false,
    });
    state
        .admin_tx
        .send(cmd.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send: {}", e)))?;

    // Disable built-in command handlers for commands this bot claims
    if let Some(ref engine) = state.command_engine {
        engine.disable_commands(&body.commands).await;
    }

    tracing::info!(
        "Bot '{}' registered, claiming commands: {:?}",
        body.nick,
        body.commands
    );
    Ok(Json(serde_json::json!({
        "status": "registered",
        "nick": body.nick,
        "commands": body.commands,
    })))
}

#[derive(Deserialize)]
pub struct BotUnregisterRequest {
    pub nick: String,
}

pub async fn unregister_bot(
    State(state): State<AppState>,
    Json(body): Json<BotUnregisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Remove from registry and collect the bot's commands
    let commands = {
        let mut bots = state.bot_registry.bots.write().await;
        match bots.remove(&body.nick) {
            Some(bot) => bot.commands.into_iter().collect::<Vec<_>>(),
            None => vec![],
        }
    };

    // Send remove_virtual_user to hub
    let cmd = serde_json::json!({
        "type": "remove_virtual_user",
        "nick": body.nick,
    });
    let _ = state.admin_tx.send(cmd.to_string()).await;

    // Re-enable built-in commands that were claimed by this bot
    if let Some(ref engine) = state.command_engine {
        engine.enable_all().await;
    }

    tracing::info!("Bot '{}' unregistered, re-enabling built-in commands", body.nick);
    Ok(Json(serde_json::json!({
        "status": "unregistered",
        "nick": body.nick,
        "commands": commands,
    })))
}

// ---------------------------------------------------------------------------
// Bot command polling and messaging
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PollQuery {
    pub nick: String,
}

pub async fn poll_commands(
    State(state): State<AppState>,
    Query(params): Query<PollQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bots = state.bot_registry.bots.read().await;
    let bot = bots.get(&params.nick).ok_or_else(|| {
        AppError::NotFound(format!("Bot '{}' not registered", params.nick))
    })?;
    let mut rx = bot.event_tx.subscribe();
    drop(bots);

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    Ok(Json(serde_json::json!({
        "events": events,
        "count": events.len(),
    })))
}

pub async fn bot_events(
    State(state): State<AppState>,
    Query(params): Query<PollQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let bots = state.bot_registry.bots.read().await;
    let bot = bots
        .get(&params.nick)
        .ok_or_else(|| AppError::NotFound(format!("Bot '{}' not registered", params.nick)))?;
    let mut rx = bot.event_tx.subscribe();
    drop(bots);

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let event_name = match &event {
                        crate::state::BotEvent::Command { .. } => "command",
                        crate::state::BotEvent::PrivateMessage { .. } => "pm",
                    };
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(Event::default().event(event_name).data(json));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    yield Ok(Event::default().event("error").data(format!("lagged by {} events", n)));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
pub struct BotChatRequest {
    pub nick: String,
    pub message: String,
}

pub async fn bot_chat(
    State(state): State<AppState>,
    Json(body): Json<BotChatRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Verify bot is registered
    {
        let bots = state.bot_registry.bots.read().await;
        if !bots.contains_key(&body.nick) {
            return Err(AppError::NotFound(format!(
                "Bot '{}' not registered",
                body.nick
            )));
        }
    }
    let cmd = serde_json::json!({
        "type": "send_chat_as",
        "nick": body.nick,
        "message": body.message,
    });
    state
        .admin_tx
        .send(cmd.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send: {}", e)))?;
    Ok(Json(serde_json::json!({"status": "sent"})))
}

#[derive(Deserialize)]
pub struct BotPmRequest {
    pub from: String,
    pub to: String,
    pub message: String,
}

pub async fn bot_pm(
    State(state): State<AppState>,
    Json(body): Json<BotPmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    {
        let bots = state.bot_registry.bots.read().await;
        if !bots.contains_key(&body.from) {
            return Err(AppError::NotFound(format!(
                "Bot '{}' not registered",
                body.from
            )));
        }
    }
    let cmd = serde_json::json!({
        "type": "send_pm_as",
        "from": body.from,
        "to": body.to,
        "message": body.message,
    });
    state
        .admin_tx
        .send(cmd.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to send: {}", e)))?;
    Ok(Json(serde_json::json!({"status": "sent"})))
}
