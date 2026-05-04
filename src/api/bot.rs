//! Bot API endpoints — external bot platform.
//!
//! Bots register via POST /api/v1/bot/register, which creates a virtual user
//! on the hub. Events are delivered via SSE on GET /api/v1/bot/events.
//! Bots respond via POST /api/v1/bot/chat or POST /api/v1/bot/pm.
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
// Token validation helper
// ---------------------------------------------------------------------------

async fn validate_bot_token(state: &AppState, nick: &str, token: &str) -> Result<(), AppError> {
    let bots = state.bot_registry.bots.read().await;
    match bots.get(nick) {
        Some(bot) if bot.token == token => Ok(()),
        Some(_) => Err(AppError::Unauthorized),
        None => Err(AppError::NotFound(format!("Bot '{}' not registered", nick))),
    }
}

// ---------------------------------------------------------------------------
// Op notification helper
// ---------------------------------------------------------------------------

async fn notify_ops(state: &AppState, message: &str) {
    let system_nick = &state.config.server.system_nick;
    let ops = state.hub_state.ops.read().await.clone();
    for op in &ops {
        let cmd = serde_json::json!({
            "type": "send_to_as",
            "nick": system_nick,
            "to": op,
            "message": message,
        });
        let _ = state.admin_tx.send(cmd.to_string()).await;
    }
}

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
    /// Event types to subscribe to on the SSE stream.
    /// Available: command, pm, chat, user_join, user_quit, user_info, kick, ban, unban,
    /// gag, ungag, hub_name, op_list, gateway_status, maintenance_tick.
    /// For command events, also register command names in the `commands` field.
    #[serde(default)]
    pub events: Vec<String>,
}

const VALID_EVENT_SUBS: &[&str] = &[
    "command",
    "pm",
    "chat",
    "user_join",
    "user_quit",
    "user_info",
    "kick",
    "ban",
    "unban",
    "gag",
    "ungag",
    "hub_name",
    "op_list",
    "gateway_status",
    "maintenance_tick",
];

pub async fn register_bot(
    State(state): State<AppState>,
    Json(body): Json<BotRegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = uuid::Uuid::new_v4().to_string();

    // Validate and collect warnings
    let mut warnings: Vec<String> = Vec::new();

    let subscribed_events: std::collections::HashSet<String> =
        body.events.iter().map(|e| e.to_lowercase()).collect();

    // Warn about unrecognized event subscriptions
    for event in &subscribed_events {
        if !VALID_EVENT_SUBS.contains(&event.as_str()) {
            warnings.push(format!("unknown event type: '{}'", event));
        }
    }

    // Warn if commands registered but 'command' not subscribed
    if !body.commands.is_empty() && !subscribed_events.contains("command") {
        warnings.push(
            "commands registered but 'command' not in events — bot will not receive command events"
                .to_string(),
        );
    }

    // Warn if 'command' subscribed but no commands registered
    if subscribed_events.contains("command") && body.commands.is_empty() {
        warnings.push(
            "'command' in events but no commands registered — no commands will be routed to this bot"
                .to_string(),
        );
    }

    // Warn if no events subscribed at all
    if subscribed_events.is_empty() && body.commands.is_empty() {
        warnings.push(
            "no events or commands registered — bot will sit silently".to_string(),
        );
    }

    for w in &warnings {
        tracing::warn!("Bot '{}' registration: {}", body.nick, w);
    }

    let has_event_subs = !subscribed_events.is_empty();
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
                token: token.clone(),
                subscribed_events,
            },
        );
    }

    // Spawn event forwarder if bot subscribes to hub events
    if has_event_subs {
        let bot_nick = body.nick.clone();
        let bot_token = token.clone();
        let bus = state.event_bus.clone();
        let registry = state.bot_registry.clone();
        tokio::spawn(async move {
            hub_event_forwarder(bus, registry, bot_nick, bot_token).await;
        });
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

    // Notify ops about the new bot registration
    notify_ops(&state, &format!("Bot '{}' registered, claiming: {:?}", body.nick, body.commands)).await;

    tracing::info!(
        "Bot '{}' registered, claiming commands: {:?}, events: {:?}",
        body.nick,
        body.commands,
        body.events,
    );

    let mut response = serde_json::json!({
        "status": "registered",
        "nick": body.nick,
        "token": token,
        "commands": body.commands,
        "events": body.events,
    });
    if !warnings.is_empty() {
        response["warnings"] = serde_json::json!(warnings);
    }
    Ok(Json(response))
}

#[derive(Deserialize)]
pub struct BotUnregisterRequest {
    pub nick: String,
    pub token: String,
}

pub async fn unregister_bot(
    State(state): State<AppState>,
    Json(body): Json<BotUnregisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate token before allowing unregister
    validate_bot_token(&state, &body.nick, &body.token).await?;

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

    // Notify ops about the bot unregistration
    notify_ops(&state, &format!("Bot '{}' unregistered", body.nick)).await;

    tracing::info!("Bot '{}' unregistered, re-enabling built-in commands", body.nick);
    Ok(Json(serde_json::json!({
        "status": "unregistered",
        "nick": body.nick,
        "commands": commands,
    })))
}

// ---------------------------------------------------------------------------
// Bot messaging and event stream
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PollQuery {
    pub nick: String,
}

/// Flatten a BotEvent into (event_type_name, json_data).
/// Hub events are unwrapped so the SSE event name is the hub event type
/// (e.g. "user_join", "kick") and the data is just the event payload.
fn flatten_bot_event(event: &crate::state::BotEvent) -> (&str, String) {
    match event {
        crate::state::BotEvent::Command {
            from_nick,
            command,
            args,
            timestamp,
        } => (
            "command",
            serde_json::json!({
                "from_nick": from_nick,
                "command": command,
                "args": args,
                "timestamp": timestamp,
            })
            .to_string(),
        ),
        crate::state::BotEvent::PrivateMessage {
            from_nick,
            message,
            timestamp,
        } => (
            "pm",
            serde_json::json!({
                "from_nick": from_nick,
                "message": message,
                "timestamp": timestamp,
            })
            .to_string(),
        ),
        crate::state::BotEvent::HubEvent { event } => {
            let tag = hub_event_type_tag(event);
            // HubEvent serializes as {"type":"X","data":{...}} — extract just the data
            let full = serde_json::to_value(event).unwrap_or_default();
            let data = full
                .get("data")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));
            (tag, data.to_string())
        }
    }
}

pub async fn bot_events(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<PollQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let token = headers
        .get("X-Bot-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    validate_bot_token(&state, &params.nick, token).await?;

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
                    let (event_name, json) = flatten_bot_event(&event);
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
    pub token: String,
}

pub async fn bot_chat(
    State(state): State<AppState>,
    Json(body): Json<BotChatRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_bot_token(&state, &body.nick, &body.token).await?;

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

    // Publish to event bus (hub no longer echoes send_chat_as back)
    state.event_bus.publish(crate::event::HubEvent::Chat {
        nick: body.nick.clone(),
        message: body.message.clone(),
        timestamp: chrono::Utc::now(),
    });

    Ok(Json(serde_json::json!({"status": "sent"})))
}

#[derive(Deserialize)]
pub struct BotPmRequest {
    pub from: String,
    pub to: String,
    pub message: String,
    pub token: String,
}

pub async fn bot_pm(
    State(state): State<AppState>,
    Json(body): Json<BotPmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_bot_token(&state, &body.from, &body.token).await?;

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

/// Map a HubEvent to its type tag for SSE event naming and subscription filtering.
fn hub_event_type_tag(event: &crate::event::HubEvent) -> &'static str {
    use crate::event::HubEvent;
    match event {
        HubEvent::Chat { .. } => "chat",
        HubEvent::UserJoin { .. } => "user_join",
        HubEvent::UserQuit { .. } => "user_quit",
        HubEvent::UserInfo { .. } => "user_info",
        HubEvent::HubName { .. } => "hub_name",
        HubEvent::OpListUpdate { .. } => "op_list",
        HubEvent::Kick { .. } => "kick",
        HubEvent::PrivateMessage { .. } => "pm",
        HubEvent::GatewayStatus { .. } => "gateway_status",
        HubEvent::Ban { .. } => "ban",
        HubEvent::Unban { .. } => "unban",
        HubEvent::Gag { .. } => "gag",
        HubEvent::Ungag { .. } => "ungag",
        HubEvent::MaintenanceTick { .. } => "maintenance_tick",
    }
}

/// Forwards matching HubEvents from the main event bus into a bot's BotEvent channel.
/// Only events the bot subscribed to at registration are forwarded.
/// Runs until the bot is unregistered (removed from registry).
/// Forwards hub events to a bot's broadcast channel.
/// Exits if the bot is unregistered or re-registered (token changes).
async fn hub_event_forwarder(
    event_bus: std::sync::Arc<crate::bus::EventBus>,
    registry: std::sync::Arc<crate::state::BotRegistry>,
    bot_nick: String,
    registration_token: String,
) {
    let mut rx = event_bus.subscribe();
    tracing::info!("Event forwarder started for bot '{}'", bot_nick);

    loop {
        match rx.recv().await {
            Ok(event) => {
                let tag = hub_event_type_tag(&event);

                let bots = registry.bots.read().await;
                let bot = match bots.get(&bot_nick) {
                    Some(b) if b.token == registration_token => b,
                    Some(_) => {
                        // Bot re-registered with a new token — a new forwarder
                        // is running for the new registration. Exit this one.
                        tracing::info!(
                            "Bot '{}' re-registered, stopping old event forwarder",
                            bot_nick
                        );
                        break;
                    }
                    None => {
                        tracing::info!(
                            "Bot '{}' unregistered, stopping event forwarder",
                            bot_nick
                        );
                        break;
                    }
                };

                if bot.subscribed_events.contains(tag) {
                    let _ = bot.event_tx.send(crate::state::BotEvent::HubEvent {
                        event: event.clone(),
                    });
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Event forwarder for '{}' lagged by {} events", bot_nick, n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
